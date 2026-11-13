//! Styled replacements for `window.confirm` and `window.alert`.
//!
//! Each Electron call site's message is kept word for word in [`messages`].
//! Where the original put `\n\n` between a question and its explanation, the
//! question is the title and the explanation the description. The confirm
//! button names the action (Delete, Remove, Download) instead of the
//! browser's OK, as the Design Guides require; Cancel stays.

use gpui_kit::component::WindowExt;
use gpui_kit::component::button::ButtonVariant;
use gpui_kit::{App, SharedString, Window};
use std::cell::RefCell;
use std::rc::Rc;

/// A question that needs a yes or no.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmRequest {
    title: SharedString,
    detail: Option<SharedString>,
    confirm: SharedString,
    destructive: bool,
}

impl ConfirmRequest {
    /// A question with a confirm button label.
    pub fn new(title: impl Into<SharedString>, confirm: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            confirm: confirm.into(),
            destructive: false,
        }
    }

    /// The explanation under the question.
    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// The action deletes or discards something.
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// The question.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// The explanation, if any.
    pub fn detail_text(&self) -> Option<&SharedString> {
        self.detail.as_ref()
    }

    /// The confirm button's label.
    pub fn confirm_label(&self) -> &SharedString {
        &self.confirm
    }

    /// Whether confirming destroys something.
    pub fn is_destructive(&self) -> bool {
        self.destructive
    }

    /// The text `window.confirm` showed, for comparison with the original.
    pub fn browser_text(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{}\n\n{}", self.title, detail),
            None => self.title.to_string(),
        }
    }
}

type Answer = Box<dyn FnOnce(bool, &mut Window, &mut App)>;

/// Asks `request` in a modal and calls `on_answer` exactly once: `true` for
/// the confirm button, `false` for Cancel, Escape or closing.
pub fn confirm(
    request: ConfirmRequest,
    window: &mut Window,
    cx: &mut App,
    on_answer: impl FnOnce(bool, &mut Window, &mut App) + 'static,
) {
    let answer: Rc<RefCell<Option<Answer>>> = Rc::new(RefCell::new(Some(Box::new(on_answer))));
    let resolve = move |value: bool, window: &mut Window, cx: &mut App| {
        if let Some(answer) = answer.borrow_mut().take() {
            answer(value, window, cx);
        }
    };
    let resolve = Rc::new(resolve);
    window.open_alert_dialog(cx, move |dialog, _, _| {
        let on_ok = resolve.clone();
        let on_cancel = resolve.clone();
        let on_close = resolve.clone();
        dialog
            .title(request.title.clone())
            .when_some_detail(request.detail.clone())
            .ok_text(request.confirm.clone())
            .ok_variant(if request.destructive {
                ButtonVariant::Danger
            } else {
                ButtonVariant::Primary
            })
            .cancel_text("Cancel")
            .show_cancel(true)
            .on_ok(move |_, window, cx| {
                on_ok(true, window, cx);
                true
            })
            .on_cancel(move |_, window, cx| {
                on_cancel(false, window, cx);
                true
            })
            .on_close(move |_, window, cx| on_close(false, window, cx))
    });
}

/// Shows `message` with one OK button, as `window.alert` did.
pub fn alert(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    let message = message.into();
    window.open_alert_dialog(cx, move |dialog, _, _| {
        dialog
            .title(message.clone())
            .ok_text("OK")
            .show_cancel(false)
    });
}

trait DetailExt {
    fn when_some_detail(self, detail: Option<SharedString>) -> Self;
}

impl DetailExt for gpui_kit::component::dialog::AlertDialog {
    fn when_some_detail(self, detail: Option<SharedString>) -> Self {
        match detail {
            Some(detail) => self.description(detail),
            None => self,
        }
    }
}

/// The messages, one function per Electron call site.
pub mod messages {
    use super::ConfirmRequest;

    fn plural(count: usize, one: &str, many: &str) -> String {
        if count == 1 {
            one.to_owned()
        } else {
            many.to_owned()
        }
    }

    /// `Lyrics.tsx`: leaving a song with unsaved edits.
    pub fn lyrics_discard_changes() -> ConfirmRequest {
        ConfirmRequest::new("Discard your unsaved changes to this song?", "Discard").destructive()
    }

    /// `Lyrics.tsx`: removing several songs from a setlist (only asked for
    /// more than one).
    pub fn lyrics_remove_songs(count: usize, setlist: &str) -> ConfirmRequest {
        ConfirmRequest::new(
            format!("Remove {count} songs from {setlist}? They stay in your library."),
            "Remove",
        )
    }

    /// `Lyrics.tsx`: deleting slides from a song.
    pub fn lyrics_delete_slides(count: usize, song: &str) -> ConfirmRequest {
        let what = if count == 1 {
            "this slide".to_owned()
        } else {
            format!("these {count} slides")
        };
        ConfirmRequest::new(format!("Delete {what} from “{song}”?"), "Delete").destructive()
    }

    /// `Settings.tsx`: downloading an offline translation. `size` is the
    /// option's `downloadApprox`, or `None` for "a few megabytes".
    pub fn settings_download_translation(
        id: &str,
        name: &str,
        size: Option<&str>,
    ) -> ConfirmRequest {
        let size = size.unwrap_or("a few megabytes");
        ConfirmRequest::new(format!("Download and use {id} ({name})?"), "Download").detail(format!(
            "Kairo will download {size} once. {id} will then work completely offline."
        ))
    }

    /// `TracksPanel.tsx`: moving a track to the Trash.
    pub fn tracks_move_to_trash(name: &str) -> ConfirmRequest {
        ConfirmRequest::new(format!("Move “{name}” to the Trash?"), "Move to Trash")
            .detail("The file leaves the audio folder, and you can restore it from the Trash.")
            .destructive()
    }

    /// `ScreenConfiguration.tsx`: deleting a screen or feed.
    pub fn screens_delete(name: &str) -> ConfirmRequest {
        ConfirmRequest::new(format!("Delete “{name}”?"), "Delete").destructive()
    }

    /// `ScreenConfiguration.tsx`: an alert when the display is taken.
    pub fn screens_display_taken(owner: &str) -> String {
        format!("{owner} is already on that display. Move it first.")
    }

    /// `ThemeEditor.tsx`: deleting a theme. `users` are the outputs that use
    /// it and keep their current look.
    pub fn theme_delete(name: &str, users: &[&str]) -> ConfirmRequest {
        let note = if users.is_empty() {
            String::new()
        } else {
            format!(" {} will keep its current look.", users.join(", "))
        };
        ConfirmRequest::new(format!("Delete “{name}”?{note}"), "Delete").destructive()
    }

    /// `PlaylistSidebar.tsx`: removing saved passages (more than one).
    pub fn playlists_remove_passages(count: usize) -> ConfirmRequest {
        ConfirmRequest::new(format!("Remove {count} saved passages?"), "Remove").destructive()
    }

    /// `PlaylistSidebar.tsx`: removing playlist items (more than one).
    pub fn playlists_remove_items(count: usize) -> ConfirmRequest {
        ConfirmRequest::new(
            format!("Remove {count} items from this playlist?"),
            "Remove",
        )
        .destructive()
    }

    /// `MediaDock.tsx`: deleting a media playlist.
    pub fn media_delete_playlist(name: &str) -> ConfirmRequest {
        ConfirmRequest::new(
            format!("Delete the “{name}” playlist? The files stay on disk."),
            "Delete",
        )
        .destructive()
    }

    /// `MediaDock.tsx`: deleting one background.
    pub fn media_delete_item(name: &str) -> ConfirmRequest {
        ConfirmRequest::new(
            format!("Delete “{name}” from the backgrounds folder?"),
            "Delete",
        )
        .detail("This removes the file from disk and cannot be undone.")
        .destructive()
    }

    /// `MediaDock.tsx`: deleting the picked backgrounds.
    pub fn media_delete_items(count: usize) -> ConfirmRequest {
        let items = plural(count, "item", "items");
        ConfirmRequest::new(
            format!("Delete {count} {items} from the backgrounds folder?"),
            "Delete",
        )
        .detail("This removes the files from disk and cannot be undone.")
        .destructive()
    }

    /// `SongUsageModal.tsx`, `Settings.tsx`, `ProgramPanels.tsx` and
    /// `useDraftPresentation.ts`: an error's message, or its text when it
    /// has none (`(err as Error).message ?? String(err)`).
    pub fn error_alert(message: Option<&str>, fallback: &str) -> String {
        match message {
            Some(message) => message.to_owned(),
            None => fallback.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::messages::*;

    #[test]
    fn messages_match_the_electron_text() {
        assert_eq!(
            lyrics_discard_changes().browser_text(),
            "Discard your unsaved changes to this song?"
        );
        assert_eq!(
            lyrics_remove_songs(3, "Sunday").browser_text(),
            "Remove 3 songs from Sunday? They stay in your library."
        );
        assert_eq!(
            lyrics_delete_slides(1, "Amazing Grace").browser_text(),
            "Delete this slide from “Amazing Grace”?"
        );
        assert_eq!(
            lyrics_delete_slides(4, "Amazing Grace").browser_text(),
            "Delete these 4 slides from “Amazing Grace”?"
        );
        assert_eq!(
            settings_download_translation("KJV", "King James Version", None).browser_text(),
            "Download and use KJV (King James Version)?\n\nKairo will download a few megabytes once. KJV will then work completely offline."
        );
        assert_eq!(
            tracks_move_to_trash("Intro.mp3").browser_text(),
            "Move “Intro.mp3” to the Trash?\n\nThe file leaves the audio folder, and you can restore it from the Trash."
        );
        assert_eq!(screens_delete("Stage").browser_text(), "Delete “Stage”?");
        assert_eq!(
            screens_display_taken("Audience"),
            "Audience is already on that display. Move it first."
        );
        assert_eq!(theme_delete("Night", &[]).browser_text(), "Delete “Night”?");
        assert_eq!(
            theme_delete("Night", &["Audience", "Stage"]).browser_text(),
            "Delete “Night”? Audience, Stage will keep its current look."
        );
        assert_eq!(
            playlists_remove_passages(2).browser_text(),
            "Remove 2 saved passages?"
        );
        assert_eq!(
            playlists_remove_items(5).browser_text(),
            "Remove 5 items from this playlist?"
        );
        assert_eq!(
            media_delete_playlist("Loops").browser_text(),
            "Delete the “Loops” playlist? The files stay on disk."
        );
        assert_eq!(
            media_delete_item("sky.jpg").browser_text(),
            "Delete “sky.jpg” from the backgrounds folder?\n\nThis removes the file from disk and cannot be undone."
        );
        assert_eq!(
            media_delete_items(1).browser_text(),
            "Delete 1 item from the backgrounds folder?\n\nThis removes the files from disk and cannot be undone."
        );
        assert_eq!(
            media_delete_items(2).browser_text(),
            "Delete 2 items from the backgrounds folder?\n\nThis removes the files from disk and cannot be undone."
        );
        assert_eq!(error_alert(Some("Disk full"), "x"), "Disk full");
        assert_eq!(error_alert(None, "TypeError"), "TypeError");
    }

    #[test]
    fn destructive_questions_are_marked() {
        assert!(screens_delete("x").is_destructive());
        assert!(!lyrics_remove_songs(2, "x").is_destructive());
        assert_eq!(tracks_move_to_trash("x").confirm_label(), "Move to Trash");
    }
}
