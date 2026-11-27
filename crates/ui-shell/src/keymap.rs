//! Shell actions, their fixed key bindings, and the customisable workspace
//! commands (`keyboard-shortcuts.ts`).
//!
//! Fixed bindings are the menu accelerators Electron's roles gave
//! (`setupApplicationMenu`) and the two global shortcuts in `App.tsx`:
//! ⌘F song search, which fires even in a text field as the original's
//! capture-phase listener does, and ⌥⌘1 Screens.
//!
//! The customisable commands (`previous`, `next`, `approve`, …) are not key
//! bindings: rebinding GPUI's keymap would also clear Kit's own text-field
//! bindings. The shell matches each key press against the operator's
//! bindings ([`command_for_keystroke`]) and skips presses in a text field.

use gpui_kit::{Action, App, KeyBinding, Keystroke, actions};
use kairo_domain::import_menu::ImportKind;
use kairo_domain::keyboard_shortcuts::{
    ShortcutKeyEvent, command_for_shortcut, shortcut_from_event,
};
use kairo_domain::settings::{ShortcutBindings, ShortcutCommand};

actions!(
    kairo_shell,
    [
        /// About Kairo (macOS application menu).
        About,
        /// Check for Updates….
        CheckForUpdates,
        /// What's New in Kairo.
        WhatsNew,
        /// Hide Kairo.
        Hide,
        /// Hide Others.
        HideOthers,
        /// Show All.
        ShowAll,
        /// Quit Kairo (asks first).
        Quit,
        /// Close Window (asks first: closing the operator window quits).
        CloseWindow,
        /// Minimize.
        Minimize,
        /// Zoom the window (the green light, or a double-click on the title area).
        ZoomWindow,
        /// Bring All to Front.
        BringAllToFront,
        /// Zoom In: a larger interface.
        ZoomIn,
        /// Zoom Out.
        ZoomOut,
        /// Actual Size.
        ActualSize,
        /// Toggle Full Screen.
        ToggleFullScreen,
        /// Search songs (⌘F).
        SongSearch,
        /// Open or close Screens (⌥⌘1).
        ToggleScreens,
        /// Open Settings.
        OpenSettings,
        /// Screens → Audience: turn every Audience screen on or off (⌘1).
        ToggleAudienceScreens,
        /// Screens → Stage: turn every Stage screen on or off (⌘2).
        ToggleStageScreens,
        /// Screens → Looks…: the header's popover choosing each screen's theme.
        OpenScreenThemes,
        /// Screens → Start or Stop Capture.
        ToggleCapture,
        /// Screens → Capture Settings…: the Live popover, where capture is set up.
        OpenCaptureSettings,
        /// View → Media Bin: open or close the media dock.
        ToggleMediaBin,
    ]
);

/// File → Import → one kind.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = kairo_shell, no_json)]
pub struct Import(pub ImportKindValue);

/// View → one workspace (⌃1–⌃9 on macOS, Alt+1–9 elsewhere).
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = kairo_shell, no_json)]
pub struct OpenWorkspace(pub RouteValue);

/// [`Route`] wrapped so it can ride an action.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteValue(pub crate::route::Route);

/// The key that opens the `index`th rail workspace from View.
pub fn workspace_key(index: usize) -> Option<&'static str> {
    const MAC: [&str; 9] = [
        "ctrl-1", "ctrl-2", "ctrl-3", "ctrl-4", "ctrl-5", "ctrl-6", "ctrl-7", "ctrl-8", "ctrl-9",
    ];
    const OTHER: [&str; 9] = [
        "alt-1", "alt-2", "alt-3", "alt-4", "alt-5", "alt-6", "alt-7", "alt-8", "alt-9",
    ];
    if cfg!(target_os = "macos") {
        MAC.get(index).copied()
    } else {
        OTHER.get(index).copied()
    }
}

/// [`ImportKind`] wrapped so it can ride an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportKindValue(pub ImportKind);

impl Default for ImportKindValue {
    fn default() -> Self {
        Self(ImportKind::Kairo)
    }
}

/// A customisable workspace command, sent to the focused workspace.
#[derive(Clone, Debug, PartialEq, Eq, Action)]
#[action(namespace = kairo_shell, no_json)]
pub struct WorkspaceCommand(pub CommandValue);

/// [`ShortcutCommand`] wrapped so it can ride an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandValue(pub ShortcutCommand);

impl Default for CommandValue {
    fn default() -> Self {
        Self(ShortcutCommand::Search)
    }
}

/// The key context of the shell's root.
pub const CONTEXT: &str = "KairoShell";

/// Installs the fixed bindings. Call once at startup.
pub fn bind(cx: &mut App) {
    // View's workspaces, in rail order: ⌃1 Operator … ⌃9 Theme Editor.
    cx.bind_keys(
        crate::route::WORKSPACES
            .iter()
            .enumerate()
            .filter_map(|(i, w)| {
                workspace_key(i)
                    .map(|key| KeyBinding::new(key, OpenWorkspace(RouteValue(w.route)), None))
            })
            .collect::<Vec<_>>(),
    );
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ActualSize, None),
        KeyBinding::new("cmd-f", SongSearch, None),
        KeyBinding::new("alt-cmd-1", ToggleScreens, None),
        KeyBinding::new("cmd-1", ToggleAudienceScreens, None),
        KeyBinding::new("cmd-2", ToggleStageScreens, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("ctrl-w", CloseWindow, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("ctrl-0", ActualSize, None),
        KeyBinding::new("ctrl-f", SongSearch, None),
        KeyBinding::new("alt-ctrl-1", ToggleScreens, None),
        KeyBinding::new("ctrl-1", ToggleAudienceScreens, None),
        KeyBinding::new("ctrl-2", ToggleStageScreens, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("f11", ToggleFullScreen, None),
    ]);
}

/// The browser `KeyboardEvent.key` for a GPUI key name, so presses match the
/// strings stored in `display.shortcuts`.
pub fn browser_key(key: &str) -> String {
    match key {
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "space" => " ".into(),
        "enter" => "Enter".into(),
        "escape" => "Escape".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "tab" => "Tab".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        key if key.starts_with('f')
            && key.len() > 1
            && key[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            key.to_uppercase()
        }
        key => key.into(),
    }
}

/// The stored shortcut string for a key press (`shortcutFromEvent`), or
/// `None` for a bare modifier.
pub fn shortcut_for_keystroke(keystroke: &Keystroke) -> Option<String> {
    let key = browser_key(&keystroke.key);
    let m = &keystroke.modifiers;
    shortcut_from_event(&ShortcutKeyEvent {
        key: &key,
        meta_key: m.platform,
        ctrl_key: m.control,
        alt_key: m.alt,
        shift_key: m.shift,
    })
}

/// The workspace command a key press triggers under the operator's
/// bindings, or `None`.
pub fn command_for_keystroke(
    keystroke: &Keystroke,
    bindings: &ShortcutBindings,
) -> Option<ShortcutCommand> {
    command_for_shortcut(&shortcut_for_keystroke(keystroke)?, bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &'static str) -> Keystroke {
        Keystroke::parse(text).unwrap()
    }

    #[test]
    fn presses_match_the_stored_strings() {
        assert_eq!(
            shortcut_for_keystroke(&key("space")).as_deref(),
            Some("Space")
        );
        assert_eq!(
            shortcut_for_keystroke(&key("pagedown")).as_deref(),
            Some("PageDown")
        );
        assert_eq!(
            shortcut_for_keystroke(&key("cmd-shift-p")).as_deref(),
            Some("Mod+Shift+p")
        );
        assert_eq!(
            shortcut_for_keystroke(&key("ctrl-shift-p")).as_deref(),
            Some("Mod+Shift+p")
        );
        assert_eq!(
            shortcut_for_keystroke(&key("left")).as_deref(),
            Some("ArrowLeft")
        );
    }

    #[test]
    fn defaults_and_custom_bindings() {
        let defaults = ShortcutBindings::default();
        assert_eq!(
            command_for_keystroke(&key("enter"), &defaults),
            Some(ShortcutCommand::Approve)
        );
        assert_eq!(
            command_for_keystroke(&key("pageup"), &defaults),
            Some(ShortcutCommand::Previous)
        );
        let custom = ShortcutBindings {
            approve: Some(vec!["Mod+p".into()]),
            clear: Some(vec![]),
            ..Default::default()
        };
        assert_eq!(command_for_keystroke(&key("space"), &custom), None);
        assert_eq!(
            command_for_keystroke(&key("cmd-p"), &custom),
            Some(ShortcutCommand::Approve)
        );
        assert_eq!(command_for_keystroke(&key("delete"), &custom), None);
        // Shift+Enter is nobody's binding.
        assert_eq!(command_for_keystroke(&key("shift-enter"), &defaults), None);
    }
}
