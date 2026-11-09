//! Operator shortcut matching and display labels.

use crate::settings::{ShortcutBindings, ShortcutCommand};

pub struct ShortcutDefinition {
    pub id: ShortcutCommand,
    pub label: &'static str,
    pub group: &'static str,
    pub defaults: &'static [&'static str],
}

pub const SHORTCUT_COMMANDS: [ShortcutDefinition; 7] = [
    ShortcutDefinition {
        id: ShortcutCommand::Search,
        label: "Focus search",
        group: "Navigation",
        defaults: &["Mod+f"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Previous,
        label: "Previous suggestion / page",
        group: "Navigation",
        defaults: &["ArrowLeft", "ArrowUp", "PageUp"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Next,
        label: "Next suggestion / page",
        group: "Navigation",
        defaults: &["ArrowRight", "ArrowDown", "PageDown"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Approve,
        label: "Present selected suggestion",
        group: "Live commands",
        defaults: &["Space", "Enter"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Dismiss,
        label: "Dismiss top suggestion",
        group: "Live commands",
        defaults: &["Escape"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Clear,
        label: "Clear live text",
        group: "Live commands",
        defaults: &["Backspace", "Delete"],
    },
    ShortcutDefinition {
        id: ShortcutCommand::Auto,
        label: "Toggle auto mode",
        group: "Live commands",
        defaults: &["Mod+a"],
    },
];

pub struct ShortcutKeyEvent<'a> {
    pub key: &'a str,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
}

pub fn shortcut_from_event(event: &ShortcutKeyEvent<'_>) -> Option<String> {
    let key = match event.key {
        "Meta" | "Control" | "Alt" | "Shift" | "Dead" | "Unidentified" => return None,
        " " => "Space".into(),
        key if key.encode_utf16().count() == 1 => key.to_lowercase(),
        key => key.to_owned(),
    };
    Some(
        [
            if event.meta_key || event.ctrl_key {
                "Mod"
            } else {
                ""
            },
            if event.alt_key { "Alt" } else { "" },
            if event.shift_key { "Shift" } else { "" },
            &key,
        ]
        .into_iter()
        .filter(|key| !key.is_empty())
        .collect::<Vec<_>>()
        .join("+"),
    )
}

pub fn bindings_for(id: ShortcutCommand, bindings: &ShortcutBindings) -> Vec<&str> {
    let custom = match id {
        ShortcutCommand::Search => &bindings.search,
        ShortcutCommand::Previous => &bindings.previous,
        ShortcutCommand::Next => &bindings.next,
        ShortcutCommand::Approve => &bindings.approve,
        ShortcutCommand::Dismiss => &bindings.dismiss,
        ShortcutCommand::Clear => &bindings.clear,
        ShortcutCommand::Auto => &bindings.auto,
    };
    match custom {
        Some(keys) => keys.iter().map(String::as_str).collect(),
        None => SHORTCUT_COMMANDS
            .iter()
            .find(|command| command.id == id)
            .unwrap()
            .defaults
            .to_vec(),
    }
}

pub fn command_for_shortcut(
    shortcut: &str,
    bindings: &ShortcutBindings,
) -> Option<ShortcutCommand> {
    SHORTCUT_COMMANDS
        .iter()
        .find(|command| bindings_for(command.id, bindings).contains(&shortcut))
        .map(|command| command.id)
}

pub fn shortcut_label(shortcut: &str) -> String {
    shortcut
        .split('+')
        .map(|key| match key {
            "Mod" => "⌘ / Ctrl".into(),
            "ArrowLeft" => "←".into(),
            "ArrowRight" => "→".into(),
            "ArrowUp" => "↑".into(),
            "ArrowDown" => "↓".into(),
            "PageUp" => "Page Up".into(),
            "PageDown" => "Page Down".into(),
            key if key.encode_utf16().count() == 1 => key.to_uppercase(),
            key => key.into(),
        })
        .collect::<Vec<String>>()
        .join(" + ")
}
