//! Control-window location, sent by GPUI to the independently supervised engine.
use gpui_kit::{App, Subscription, Window};
use kairo_core::engine::EngineHandle;

pub struct ControlDisplayReporter {
    engine: EngineHandle,
    _subscription: Subscription,
}
impl ControlDisplayReporter {
    pub fn new(window: &Window, cx: &App, engine: EngineHandle) -> Self {
        let reporter = engine.clone();
        let subscription =
            window.observe_window_display(move |window, cx| report(&reporter, window, cx));
        report(&engine, window, cx);
        Self {
            engine,
            _subscription: subscription,
        }
    }
}
fn report(engine: &EngineHandle, window: &Window, cx: &App) {
    engine.report_control_display(window.display(cx).map(|display| u64::from(display.id())));
}
impl Drop for ControlDisplayReporter {
    fn drop(&mut self) {
        self.engine.report_control_display(None);
    }
}
