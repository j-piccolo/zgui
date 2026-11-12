//! A renderer that lost its composed picture is drawn for in full.
//!
//! A renderer that keeps the last frame can lose it with no configure to say so: an acquisition
//! that came back outdated, a device rebuilt after a loss. Its next draw clears and redraws every
//! pixel. The display list holds only what the frame's damage reached, so a frame built for one
//! changed node would leave the surface cleared with that node, and the boxes behind it, drawn
//! back alone.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use zgui_bits::DamageSet;
use zgui_platform::Surface;
use zgui_reactive::RwSignal;
use zgui_reactive::prelude::{Get, Set};
use zgui_render::{
    ExternalTexture, FrameOutcome, FrameStats, MemoryReport, RenderCapabilities, RenderTarget,
    Renderer, TextureHandle,
};
use zgui_runtime::{App, AppError, Runtime};
use zgui_scene::Scene;
use zgui_view::{BuildCx, IntoView, View};

/// A window far larger than the swatch that changes in it, with a second swatch that does not.
const CSS: &str = "root { display: block; width: 400px; height: 300px; padding: 100px }
                   .swatch { display: block; width: 30px; height: 20px; background-color: #101010 }
                   .swatch.lit { background-color: #f0f0f0 }
                   .still { display: block; width: 30px; height: 20px; background-color: #808080 }";

/// What one frame was drawn against.
#[derive(Debug, Clone, Copy)]
struct Drawn {
    /// Whether the damage was the whole surface.
    full: bool,
    /// How many quads the display list held.
    quads: usize,
}

/// A renderer that records each frame and can say it lost its picture.
struct Losing {
    /// The frames, in order.
    log: Rc<RefCell<Vec<Drawn>>>,
    /// Whether the next draw redraws everything, as after an outdated acquisition.
    lost: Rc<Cell<bool>>,
    /// The surface it was pointed at.
    target: Option<RenderTarget>,
    /// Tiles go into plain memory.
    atlas: zgui_atlas::MemorySink,
}

impl Renderer for Losing {
    fn capabilities(&self) -> RenderCapabilities {
        RenderCapabilities::MINIMAL
    }

    fn configure(&mut self, target: RenderTarget) {
        self.target = Some(target);
    }

    fn target(&self) -> Option<RenderTarget> {
        self.target
    }

    fn composed_survives_configure(&self) -> bool {
        true
    }

    fn owes_whole_surface(&self) -> bool {
        self.lost.get()
    }

    fn draw(&mut self, scene: &Scene, damage: &DamageSet) -> FrameOutcome {
        self.lost.set(false);
        self.log.borrow_mut().push(Drawn {
            full: damage.is_full(),
            quads: scene.primitives.quads.len(),
        });
        FrameOutcome::Presented(FrameStats {
            vector_passes: 0,
            draw_calls: 0,
            damage_px: 0,
            bytes_uploaded: 0,
            memory: MemoryReport::ZERO,
        })
    }

    fn register_external(&mut self, _texture: ExternalTexture) -> TextureHandle {
        TextureHandle(0)
    }

    fn release_external(&mut self, _handle: TextureHandle) {}

    fn memory(&self) -> MemoryReport {
        MemoryReport::ZERO
    }

    fn texture_sink(&mut self) -> &mut dyn zgui_atlas::TextureSink {
        &mut self.atlas
    }
}

/// A window with one swatch `lit` recolours and one that never changes.
fn window(
    lit: RwSignal<bool>,
    log: &Rc<RefCell<Vec<Drawn>>>,
    lost: &Rc<Cell<bool>>,
) -> zgui_platform_headless::Harness<Runtime> {
    let (log, lost) = (Rc::clone(log), Rc::clone(lost));
    let handler = App::new()
        .with_title("whole redraw")
        .with_size(400.0, 300.0)
        .with_stylesheet(CSS)
        .with_renderer(Box::new(
            move |_surface: &Arc<dyn Surface>, target: RenderTarget| {
                let mut renderer = Losing {
                    log: Rc::clone(&log),
                    lost: Rc::clone(&lost),
                    target: None,
                    atlas: zgui_atlas::MemorySink::default(),
                };
                renderer.configure(target);
                Ok::<Box<dyn Renderer>, AppError>(Box::new(renderer))
            },
        ))
        .into_handler(move |cx: &mut BuildCx<'_>| {
            Box::new(
                zgui_elements::column()
                    .class("root")
                    .child(zgui_elements::column().class("still"))
                    .child(
                        zgui_elements::column()
                            .class("swatch")
                            .class_toggle(zgui_view::ClassName::new("lit"), move || lit.get()),
                    )
                    .into_view()
                    .build(cx),
            ) as Box<dyn zgui_view::Anchor>
        })
        .expect("the reactive runtime installs");
    zgui_platform_headless::Harness::new(handler)
}

#[test]
fn a_change_after_the_renderer_lost_its_picture_is_drawn_with_the_whole_document() {
    let lit = RwSignal::new(false);
    let log = Rc::new(RefCell::new(Vec::new()));
    let lost = Rc::new(Cell::new(false));
    let mut app = window(lit, &log, &lost);
    app.settle(8);
    let whole = log
        .borrow()
        .first()
        .copied()
        .expect("mounting drew a frame");
    assert!(whole.full, "the first frame was not the whole surface");

    // The control: with the picture kept, one swatch changing draws less than the document.
    log.borrow_mut().clear();
    lit.set(true);
    app.settle(8);
    let narrow = log.borrow().first().copied().expect("the change drew");
    assert!(
        !narrow.full && narrow.quads < whole.quads,
        "a single swatch redrew the document, so the case below would show nothing: {narrow:?}"
    );

    // The picture is lost, and the same change follows it.
    log.borrow_mut().clear();
    lost.set(true);
    lit.set(false);
    app.settle(8);
    let after = log.borrow().first().copied().expect("the change drew");
    assert!(
        after.full,
        "the renderer redraws everything and was handed a frame built for less: {after:?}"
    );
    assert_eq!(
        after.quads, whole.quads,
        "the display list left out boxes the cleared surface needs drawn back"
    );
}
