//! Native UI bootstrap window.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;

struct HelloWorld;

impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child("Hello, world!")
    }
}

fn main() {
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        open_window(WindowOptions::default(), cx, |_, cx| cx.new(|_| HelloWorld))
            .expect("failed to open the Kairo bootstrap window");

        cx.activate(true);
    });
}
