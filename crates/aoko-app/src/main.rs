use gpui::prelude::*;
use gpui::{
    div, px, size, App, Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions,
};

struct AokoShell;

impl Render for AokoShell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(gpui::rgb(0x0a0b0f))
    }
}

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1260.0), px(820.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_window, cx| cx.new(|_| AokoShell),
        )
        .expect("open window");
    });
}
