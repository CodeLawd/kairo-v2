//! Accessibility audit of the shell (prompt 106): the top bar, the workspace
//! rail and the shell's own overlays, read from the AccessKit tree VoiceOver
//! and Narrator see. Its own binary: `a11y_audit::activate` must run before
//! the process's first window.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, TestAppContext, Window, WindowBounds,
    WindowOptions, px, size,
};
use kairo_ui_kit::a11y_audit::{self, A11yTree};
use ui_shell::{Route, Shell, ShellOptions};

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    a11y_audit::activate();
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
        ui_shell::keymap::bind(cx);
    });
    let options = ShellOptions {
        version: "2.0.0".into(),
        dev: false,
        user_data: None,
        splash: false,
    };
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| Shell::new(options, window, cx)),
        )
        .expect("open shell")
    });
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |s, cx| s.mark_ready(window, cx));
        shell.read(cx).focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
    })
    .unwrap();
    (handle, shell)
}

fn tree(cx: &mut TestAppContext, handle: AnyWindowHandle) -> A11yTree {
    cx.update_window(handle, |_, window: &mut Window, cx| {
        window.render_frame(cx);
        A11yTree::of(window)
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_top_bar_and_rail_are_named_and_pressable(cx: &mut TestAppContext) {
    let (handle, shell) = open(cx);
    let t = tree(cx, handle);
    t.assert_accessible();

    // Landmarks, as Electron's `<header>`, `<nav>` and `<main>`.
    assert_eq!(t.named("Workspaces").role, "Navigation");
    assert_eq!(t.named("Presentation controls").role, "Toolbar");
    assert_eq!(t.with_role("Main").len(), 1);

    // Every rail button, in rail order, and the current one is selected.
    let rail: Vec<_> = [
        "Operator", "Bible", "Lyrics", "Slides", "Engage", "Web", "Theme",
    ]
    .iter()
    .map(|name| {
        t.with_role("Button")
            .into_iter()
            .find(|n| n.label.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("rail button {name}: {:?}", t.names()))
    })
    .collect();
    assert!(rail.iter().all(|n| n.pressable() && n.focusable()));
    assert_eq!(rail[0].selected, Some(true));
    assert!(rail[1..].iter().all(|n| n.selected != Some(true)));

    for route in [Route::Scripture, Route::Lyrics, Route::Theme] {
        cx.update(|cx| shell.update(cx, |s, cx| s.navigate(route, cx)));
        let t = tree(cx, handle);
        t.assert_accessible();
        let selected: Vec<_> = t
            .with_role("Button")
            .into_iter()
            .filter(|n| n.selected == Some(true))
            .filter_map(|n| n.label.clone())
            .collect();
        assert_eq!(selected.len(), 1, "{route:?}: {selected:?}");
    }

    // The footer's popover buttons open from VoiceOver as well as the mouse.
    for name in ["Feedback", "Audio", "Account", "Settings"] {
        assert!(t.named(name).pressable(), "{name}");
    }
}

#[gpui_kit::test]
fn rail_popovers_still_open_with_the_mouse(cx: &mut TestAppContext) {
    let (handle, _) = open(cx);
    for button in ["audio", "account", "feedback"] {
        assert!(tree(cx, handle).with_role("Dialog").is_empty());
        cx.update_window(handle, |_, window, cx| window.click(button, cx))
            .unwrap();
        // An open Kit popover is a non-modal dialog node.
        assert_eq!(
            tree(cx, handle).with_role("Dialog").len(),
            1,
            "{button} popover did not open"
        );
        cx.update_window(handle, |_, window, cx| window.press("escape", cx))
            .unwrap();
    }
}

/// Radix `AlertDialog`: focus moves in, Tab stays inside, Escape closes and
/// focus goes back to what had it.
#[gpui_kit::test]
fn the_quit_question_holds_focus_and_gives_it_back(cx: &mut TestAppContext) {
    let (handle, shell) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(ui_shell::keymap::Quit), cx);
    })
    .unwrap();
    let t = tree(cx, handle);
    assert_eq!(t.named("Quit Kairo?").role, "AlertDialog");
    assert!(t.focus_within("Quit Kairo?"), "focus moved into the dialog");
    // DESIGN.md: Keep Kairo open receives initial focus.
    assert_eq!(
        t.focused().and_then(|n| n.label.as_deref()),
        Some("Keep Kairo open")
    );
    for _ in 0..6 {
        cx.update_window(handle, |_, window, cx| window.press("tab", cx))
            .unwrap();
        assert!(
            tree(cx, handle).focus_within("Quit Kairo?"),
            "Tab left the dialog"
        );
    }
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    let t = tree(cx, handle);
    assert!(t.with_role("AlertDialog").is_empty());
    let back = cx
        .update_window(handle, |_, window, cx| {
            shell.read(cx).focus_handle(cx).is_focused(window)
        })
        .unwrap();
    assert!(back, "focus went back to the shell");
}
