//! A desktop bar: one shell layer per output, with blur behind it and a panel it opens.
//!
//! Run it under a wlroots-style compositor with `cargo run -p zgui-examples --example layer`.

use zgui::platform::{KeyboardInteractivity, Layer, LayerPlacement, SurfaceRole};
use zgui::prelude::*;
use zgui::reactive::RenderEffect;
use zgui::geom::{CssPx, Edges, Point, Rect, Size};

const WIDTH: f32 = 48.0;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect<CssPx, zgui::geom::Css> {
    Rect::new(Point::new(CssPx(x), CssPx(y)), Size::new(CssPx(w), CssPx(h)))
}

#[component]
fn Bar(panel: RwSignal<bool>) -> impl IntoView {
    let this = use_window();
    // Blur behind the whole bar, following its size.
    let blur = RenderEffect::new({
        let this = this.clone();
        move |_| {
            let size = this.size().get();
            this.set_blur_region(Some(&[rect(0.0, 0.0, size.width.0, size.height.0)]));
        }
    });
    core::mem::forget(blur);
    view! {
        column(class = "bar") {
            control(class = "dot", on:click = move |_| panel.update(|open| *open = !*open))
            box(class = "dot dot--b")
            spacer()
            label(class = "clock") {"12\n34"}
        }
    }
}

#[component]
fn Panel() -> impl IntoView {
    view! {
        column(class = "panel") {
            label(class = "title") {"Control Center"}
            label(class = "note") {"a second layer surface"}
        }
    }
}

const SHEET: &str = css!(
    ":root { background-color: transparent; color: #e0def4; font-family: sans-serif; }
    .bar { width: 100%; height: 100%; align-items: center; padding: 10px 0; gap: 10px;
           background-color: rgba(25, 23, 36, 0.55); }
    .dot { width: 28px; height: 28px; border-radius: 14px; background-color: #c4a7e7;
           transition: transform 150ms ease-out, background-color 150ms; }
    .dot:hover { background-color: #ebbcba; }
    .dot:active { transform: scale(0.9); }
    .dot--b { background-color: #31748f; }
    .clock { font-size: 13px; font-weight: 600; text-align: center; }
    .panel { width: 100%; height: 100%; padding: 18px; gap: 6px; border-radius: 18px;
             background-color: rgba(25, 23, 36, 0.7); border: 1px solid rgba(255,255,255,0.12);
             animation: reveal 220ms ease-out; }
    .title { font-size: 17px; font-weight: 700; }
    .note { font-size: 12px; color: #908caa; }
    @keyframes reveal { from { opacity: 0; transform: translateX(-12px); } to { opacity: 1; } }"
);

fn main() -> Result<(), zgui::Error> {
    app()
        .with_application_id("dev.zgui.Layer")
        .with_stylesheet(SHEET)
        .with_no_window()
        .with_context(|| {
            let windows = use_windows();
            let panel = RwSignal::new(false);
            let bars = RenderEffect::new({
                let windows = windows.clone();
                move |opened: Option<Vec<(String, WindowHandle)>>| {
                    let mut opened = opened.unwrap_or_default();
                    let monitors = windows.monitors().get();
                    opened.retain(|(name, handle)| {
                        let keep = monitors.iter().any(|m| m.name.as_ref().is_some_and(|held| held.as_str() == name.as_str()));
                        if !keep {
                            handle.close();
                        }
                        keep
                    });
                    for monitor in monitors {
                        let Some(name) = monitor.name.as_ref().map(ToString::to_string) else {
                            continue;
                        };
                        if opened.iter().any(|(held, _)| *held == name) {
                            continue;
                        }
                        let placement = LayerPlacement {
                            layer: Layer::Top,
                            anchors: Edges::new(true, false, true, true),
                            exclusive_zone: Some(CssPx(WIDTH)),
                            keyboard: KeyboardInteractivity::OnDemand,
                            monitor: Some(name.clone()),
                            namespace: Some("zgui-bar".into()),
                            ..LayerPlacement::default()
                        };
                        let handle = windows.open(
                            WindowOptions::new("bar")
                                .with_role(SurfaceRole::Layer(placement))
                                .with_transparent(true)
                                .with_size(WIDTH, 400.0),
                            move || view! { Bar(panel = panel) },
                        );
                        opened.push((name, handle));
                    }
                    opened
                }
            });
            core::mem::forget(bars);
            let sheet = RenderEffect::new(move |held: Option<Option<WindowHandle>>| {
                let held = held.flatten();
                match (panel.get(), held) {
                    (true, None) => {
                        let placement = LayerPlacement {
                            layer: Layer::Overlay,
                            anchors: Edges::new(true, false, false, true),
                            margin: Edges::new(CssPx(12.0), CssPx(0.0), CssPx(0.0), CssPx(12.0)),
                            exclusive_zone: Some(CssPx(0.0)),
                            namespace: Some("zgui-panel".into()),
                            ..LayerPlacement::default()
                        };
                        let handle = windows.open(
                            WindowOptions::new("panel")
                                .with_role(SurfaceRole::Layer(placement))
                                .with_transparent(true)
                                .with_size(320.0, 220.0),
                            || view! { Panel() },
                        );
                        Some(handle)
                    }
                    (false, Some(handle)) => {
                        handle.close();
                        None
                    }
                    (_, held) => held,
                }
            });
            core::mem::forget(sheet);
        })
        .run(|| view! { column() })
}
