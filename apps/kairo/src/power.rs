//! System wake notifications. Window focus is raised by the shell separately.
use crate::services::ApplicationServices;
use gpui_kit::*;
use std::sync::Arc;

#[cfg(target_os = "macos")]
struct WakeObserver {
    center: objc2::rc::Retained<objc2_foundation::NSNotificationCenter>,
    observer:
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
}
#[cfg(target_os = "macos")]
impl Global for WakeObserver {}
#[cfg(target_os = "macos")]
impl Drop for WakeObserver {
    fn drop(&mut self) {
        // SAFETY: the retained token was issued by this center and is removed
        // before either retained Objective-C object is dropped.
        unsafe { self.center.removeObserver((*self.observer).as_ref()) };
    }
}

#[cfg(target_os = "macos")]
pub fn observe(services: Arc<ApplicationServices>, cx: &mut App) {
    use block2::RcBlock;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceDidWakeNotification};
    use objc2_foundation::NSNotification;
    use std::ptr::NonNull;
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let weak = Arc::downgrade(&services);
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(services) = weak.upgrade() {
            services
                .core
                .publisher()
                .emit(kairo_core::PowerEvent::Resume);
            let cloud = services.cloud.clone();
            services.spawn(async move {
                cloud.reconnect().await;
            });
        }
    });
    // SAFETY: AppKit notification name and center are framework-owned. The
    // copied callback captures only Send+Sync handles and accepts no object.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidWakeNotification),
            None,
            None,
            &block,
        )
    };
    cx.set_global(WakeObserver { center, observer });
}

#[cfg(not(target_os = "macos"))]
pub fn observe(services: Arc<ApplicationServices>, cx: &mut App) {
    // A cross-platform resume fallback: monotonic clocks that exclude suspend
    // diverge from wall time on wake; those including suspend show a long tick.
    cx.spawn(async move |cx| {
        let mut monotonic = std::time::Instant::now();
        let mut wall = std::time::SystemTime::now();
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(1))
                .await;
            let now = std::time::Instant::now();
            let wall_now = std::time::SystemTime::now();
            let elapsed = now.duration_since(monotonic);
            let wall_elapsed = wall_now.duration_since(wall).unwrap_or_default();
            if elapsed > std::time::Duration::from_secs(5)
                || wall_elapsed.saturating_sub(elapsed) > std::time::Duration::from_secs(2)
            {
                services
                    .core
                    .publisher()
                    .emit(kairo_core::PowerEvent::Resume);
                let cloud = services.cloud.clone();
                services.spawn(async move {
                    cloud.reconnect().await;
                });
            }
            monotonic = now;
            wall = wall_now;
        }
    })
    .detach();
}
