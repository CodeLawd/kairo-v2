//! Deliver core topic updates to GPUI entities on the foreground executor.

use std::sync::Arc;

pub mod preview;
pub mod thumbnails;

use futures_channel::oneshot;
use gpui_kit::{Context, Task, Window};
use kairo_core::{EventStream, StreamError, Watch};

/// Retain the task in the owning view. Drop it to unsubscribe.
pub fn watch<View: 'static, T: 'static>(
    cx: &Context<View>,
    mut subscription: Watch<T>,
    mut apply: impl FnMut(&mut View, Arc<T>, &mut Context<View>) + 'static,
) -> Task<()> {
    cx.spawn(async move |view, cx| {
        while let Some(value) = subscription.next().await {
            if view
                .update(cx, |view, cx| {
                    apply(view, value, cx);
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        }
    })
}

/// Sample the latest state at most once per display frame. A frame is
/// requested only when a new value arrives. Drop the task to cancel.
pub fn watch_frame<View: 'static, T: 'static>(
    window: &Window,
    cx: &Context<View>,
    mut subscription: Watch<T>,
    mut apply: impl FnMut(&mut View, Arc<T>, &mut Context<View>) + 'static,
) -> Task<()> {
    cx.spawn_in(window, async move |view, cx| {
        while subscription.next().await.is_some() {
            let (ready, frame) = oneshot::channel();
            cx.on_next_frame(move |_, _| {
                let _ = ready.send(());
            });
            if frame.await.is_err() {
                break;
            }
            if let Some(value) = subscription.latest()
                && view
                    .update_in(cx, |view, _, cx| {
                        apply(view, value, cx);
                        cx.notify();
                    })
                    .is_err()
            {
                break;
            }
        }
    })
}

/// Deliver retained events in order. A lag error lets the view recover from
/// a gap before consuming the next retained event.
pub fn stream<View: 'static, T: 'static>(
    cx: &Context<View>,
    mut subscription: EventStream<T>,
    mut apply: impl FnMut(&mut View, Result<Arc<T>, StreamError>, &mut Context<View>) + 'static,
) -> Task<()> {
    cx.spawn(async move |view, cx| {
        while let Some(event) = subscription.next().await {
            if view
                .update(cx, |view, cx| {
                    apply(view, event, cx);
                    cx.notify();
                })
                .is_err()
            {
                break;
            }
        }
    })
}
