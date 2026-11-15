//! Startup failures the operator must see (prompt 103).
//!
//! When Kairo cannot open a window there is nothing of its own to draw a
//! message with: no X display, or no graphics driver that Vulkan or OpenGL can
//! use. On Linux the message goes to the desktop's dialog tool (`zenity`,
//! then `kdialog`), else a notification (`notify-send`). It is always written
//! to standard error and the log as well, so a terminal launch shows it too.

/// No X display: under Wayland that means XWayland is missing or off.
#[cfg_attr(
    not(target_os = "linux"),
    expect(dead_code, reason = "the display check is Linux only")
)]
pub const NO_DISPLAY_TITLE: &str = "Kairo needs an X11 display";
#[cfg_attr(
    not(target_os = "linux"),
    expect(dead_code, reason = "the display check is Linux only")
)]
pub const NO_DISPLAY_TEXT: &str = "Kairo could not find an X11 display (DISPLAY is not set).\n\n\
Kairo runs on X11, or on XWayland in a Wayland session. Start Kairo from your desktop, \
or install and turn on XWayland, then try again.";

/// The operator window could not be opened.
pub const NO_WINDOW_TITLE: &str = "Kairo could not open its window";

/// The text for a window that failed to open, with the cause.
pub fn no_window_text(error: &str) -> String {
    format!(
        "Kairo could not open its window. It needs a graphics driver that supports \
Vulkan or OpenGL.\n\nInstall or update your graphics driver (on Ubuntu, the \
mesa-vulkan-drivers package, or your GPU maker's driver), then start Kairo again.\n\n\
Details: {error}"
    )
}

/// The programs tried in order to show a message, each with its arguments.
pub fn dialog_commands(title: &str, text: &str) -> Vec<(&'static str, Vec<String>)> {
    vec![
        (
            "zenity",
            vec![
                "--error".into(),
                "--no-markup".into(),
                format!("--title={title}"),
                format!("--text={text}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".into(),
                title.into(),
                "--error".into(),
                text.into(),
            ],
        ),
        (
            "notify-send",
            vec![
                "--urgency=critical".into(),
                "--app-name=Kairo".into(),
                title.into(),
                text.into(),
            ],
        ),
    ]
}

/// Shows `title` and `text` to the operator as well as the platform allows.
pub fn report(title: &str, text: &str) {
    tracing::error!(title, text, "[Startup] cannot start");
    eprintln!("{title}\n\n{text}");
    if cfg!(target_os = "linux") {
        for (program, args) in dialog_commands(title, text) {
            // A dialog tool that is missing fails to spawn; try the next one.
            if let Ok(status) = std::process::Command::new(program).args(&args).status()
                && status.success()
            {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_are_tried_in_order_with_the_whole_message() {
        let commands = dialog_commands("Title", "Line one\nLine two");
        let programs: Vec<_> = commands.iter().map(|(p, _)| *p).collect();
        assert_eq!(programs, ["zenity", "kdialog", "notify-send"]);
        for (_, args) in &commands {
            assert!(args.iter().any(|a| a.contains("Title")));
            assert!(args.iter().any(|a| a.contains("Line one\nLine two")));
        }
        // zenity must not read the text as Pango markup.
        assert!(commands[0].1.contains(&"--no-markup".to_string()));
    }

    #[test]
    fn the_window_message_names_the_drivers_and_the_cause() {
        let text = no_window_text("No GPU adapter found that can configure the display surface");
        assert!(text.contains("Vulkan or OpenGL"));
        assert!(text.contains("mesa-vulkan-drivers"));
        assert!(text.ends_with("No GPU adapter found that can configure the display surface"));
    }
}
