//! The activation panel as a modal (`ActivationDialog.tsx`), from the
//! header chip or a feature that needs activation.

use crate::license::LicensePanel;
use crate::model::Account;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::v_flex;
use gpui_kit::{
    AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, Role, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div, px,
    size,
};
use kairo_ui_kit::components::modal::{MovableDialog, MovableDialogEvent};
use kairo_ui_kit::tokens::{TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// Element ids, for tests.
pub mod ids {
    /// Close.
    pub const CLOSE: &str = "activation-close";
}

/// Closing the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DismissActivation;

/// The dialog.
pub struct ActivationDialog {
    frame: Entity<MovableDialog>,
    panel: Entity<LicensePanel>,
    title: SharedString,
    description: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DismissActivation> for ActivationDialog {}

impl ActivationDialog {
    /// The dialog, with Electron's default title and description unless given.
    pub fn new(
        account: Entity<Account>,
        title: Option<SharedString>,
        description: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = title.unwrap_or_else(|| "Activate this computer".into());
        let frame = cx.new(|cx| {
            MovableDialog::new(
                "activation-dialog",
                title.clone(),
                size(px(448.), px(236.)),
                window,
                cx,
            )
        });
        let panel = cx.new(|cx| LicensePanel::new(account.clone(), true, window, cx));
        let subscriptions = vec![
            cx.subscribe(&frame, |_, _, event: &MovableDialogEvent, cx| {
                if matches!(event, MovableDialogEvent::Dismiss) {
                    cx.emit(DismissActivation);
                }
            }),
            // Activated: nothing left to do here.
            cx.observe(&account, |_, account, cx| {
                if account.read(cx).license().is_some_and(|l| l.activated) {
                    cx.notify();
                }
            }),
        ];
        Self {
            frame,
            panel,
            title,
            description: description.unwrap_or_else(|| {
                "Turns on live transcription, scripture lookup and lyric tools.".into()
            }),
            _subscriptions: subscriptions,
        }
    }
}

impl Render for ActivationDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let handle = self
            .frame
            .update(cx, |frame, cx| frame.drag_handle("activation-drag", cx));
        let content = v_flex()
            .relative()
            .size_full()
            .p_6()
            .child(
                handle.pr_8().child(
                    div()
                        .mb_2()
                        .text_size(px(17.))
                        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                        .text_color(k.text_primary)
                        .child(self.title.clone()),
                ),
            )
            .child(
                div()
                    .mb(px(20.))
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T400))
                    .child(self.description.clone()),
            )
            .child(self.panel.clone())
            .child(
                div()
                    .id(ids::CLOSE)
                    .test_support()
                    .role(Role::Button)
                    .aria_label("Close")
                    .absolute()
                    .right_3()
                    .top_3()
                    .size_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .cursor_pointer()
                    .text_color(k.text(TextStep::T400))
                    .hover(move |s| s.bg(k.white.opacity(0.1)).text_color(k.text_primary))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissActivation)))
                    .child(gpui_kit::component::Icon::new(KairoIcon::X).size(px(14.))),
            );
        self.frame
            .update(cx, |frame, cx| frame.frame(content, window, cx))
    }
}
