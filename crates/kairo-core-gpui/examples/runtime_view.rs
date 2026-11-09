//! Run with cargo run -p kairo-core-gpui --example runtime_view.
//! Pass --verify to exercise the view and exit after all four paths deliver.

use std::{num::NonZeroUsize, time::Duration};

use gpui_kit::{
    AppContext, Context, IntoElement, Render, Task, Window, WindowOptions,
    component::{ActiveTheme, button::Button},
    div,
    prelude::*,
};
use kairo_core::{AppCore, Handle, Message, Publisher};

struct Counter {
    value: u32,
    topics: Publisher,
}
struct Count;
struct Increment;
struct Incremented(u32);

impl Message for Count {
    type Actor = Counter;
}
impl Message for Increment {
    type Actor = Counter;
}
impl Handle<Count> for Counter {
    type Reply = u32;
    async fn handle(&mut self, _: Count) -> u32 {
        self.value
    }
}
impl Handle<Increment> for Counter {
    type Reply = ();
    async fn handle(&mut self, _: Increment) {
        self.value += 1;
        self.topics.publish(self.value);
        self.topics.emit(Incremented(self.value));
    }
}

struct RuntimeView {
    core: AppCore,
    count: u32,
    event: u32,
    reply: u32,
    error: Option<String>,
    verify: bool,
    _watch: Task<()>,
    _stream: Task<()>,
    call: Task<()>,
}

impl RuntimeView {
    fn new(core: AppCore, verify: bool, window: &Window, cx: &mut Context<Self>) -> Self {
        let watch = kairo_core_gpui::watch_frame(
            window,
            cx,
            core.watch::<u32>(),
            |view: &mut Self, value, cx| {
                view.count = *value;
                view.finish_verification(cx);
            },
        );
        let stream = kairo_core_gpui::stream(
            cx,
            core.stream::<Incremented>(),
            |view: &mut Self, value, cx| {
                match value {
                    Ok(event) => view.event = event.0,
                    Err(error) => view.error = Some(error.to_string()),
                }
                view.finish_verification(cx);
            },
        );
        let mut view = Self {
            core,
            count: 0,
            event: 0,
            reply: 0,
            error: None,
            verify,
            _watch: watch,
            _stream: stream,
            call: Task::ready(()),
        };
        if verify {
            view.increment(cx);
        }
        view
    }

    fn increment(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = self.core.send(Increment) {
            self.error = Some(error.to_string());
            cx.notify();
            return;
        }
        let core = self.core.clone();
        self.call = cx.spawn(async move |view, cx| {
            let reply = core.call(Count).await;
            let _ = view.update(cx, |view, cx| {
                match reply {
                    Ok(count) => view.reply = count,
                    Err(error) => view.error = Some(error.to_string()),
                }
                view.finish_verification(cx);
                cx.notify();
            });
        });
    }

    fn finish_verification(&self, cx: &mut Context<Self>) {
        if self.verify && (self.count, self.event, self.reply) == (1, 1, 1) {
            println!("GPUI runtime view verified send, call, frame watch and ordered stream");
            cx.quit();
        }
    }
}

impl Render for RuntimeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(format!(
                "Snapshot {}, event {}, reply {}",
                self.count, self.event, self.reply
            ))
            .child(
                Button::new("increment")
                    .label("Increment")
                    .on_click(cx.listener(|view, _, _, cx| view.increment(cx))),
            )
            .children(self.error.clone())
    }
}

fn main() {
    let verify = std::env::args().any(|arg| arg == "--verify");
    let core = AppCore::new().expect("start core runtime");
    core.register(
        Counter {
            value: 0,
            topics: core.publisher(),
        },
        NonZeroUsize::new(16).unwrap(),
    )
    .expect("register sample counter");

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| RuntimeView::new(core, verify, window, cx))
        })
        .expect("open runtime view");
        if verify {
            cx.spawn(async |cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                eprintln!("Timed out waiting for the GPUI runtime view");
                std::process::exit(1);
            })
            .detach();
        }
        cx.activate(true);
    });
}
