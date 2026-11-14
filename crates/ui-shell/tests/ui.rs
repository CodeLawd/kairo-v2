//! UI integration tests for the shell: navigation, shortcuts, quit paths,
//! imports, overlays, the interface scale, What's New and feedback.

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Focusable, Global, SharedString, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};
use kairo_domain::import_menu::ImportKind;
use kairo_domain::settings::{ShortcutBindings, ShortcutCommand};
use ui_shell::keymap::{
    ActualSize, Import, ImportKindValue, Quit, SongSearch, ToggleScreens, WorkspaceCommand, ZoomIn,
    ZoomOut,
};
use ui_shell::state::{AccountView, UpdateState};
use ui_shell::{Overlay, Route, Shell, ShellOptions, ShellRequest, ShellState};

#[derive(Default)]
struct Seen {
    requests: Vec<ShellRequest>,
    commands: Vec<ShortcutCommand>,
}
impl Global for Seen {}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
        ui_shell::keymap::bind(cx);
        cx.set_global(Seen::default());
        // A workspace that takes commands (prompt 076 builds the real ones).
        cx.on_action(|action: &WorkspaceCommand, cx| {
            cx.global_mut::<Seen>().commands.push(action.0.0)
        });
    });
}

fn open(cx: &mut TestAppContext, version: &str) -> (AnyWindowHandle, Entity<Shell>) {
    let options = ShellOptions {
        version: version.into(),
        dev: false,
        user_data: None,
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
    cx.update(|cx| {
        cx.subscribe(&shell, |_, request: &ShellRequest, cx| {
            cx.global_mut::<Seen>().requests.push(request.clone())
        })
        .detach();
    });
    (handle, shell)
}

fn ready(cx: &mut TestAppContext, handle: AnyWindowHandle, shell: &Entity<Shell>) {
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |s, cx| s.mark_ready(window, cx));
        shell.read(cx).focus_handle(cx).focus(window, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn requests(cx: &mut TestAppContext) -> Vec<ShellRequest> {
    cx.update(|cx| cx.global::<Seen>().requests.clone())
}

fn dispatch(cx: &mut TestAppContext, handle: AnyWindowHandle, action: impl gpui_kit::Action) {
    cx.update_window(handle, |_, window: &mut Window, cx| {
        window.dispatch_action(Box::new(action), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_rail_navigates_every_workspace(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find("app-workspace").label(), Some("Operator"));
        for w in ui_shell::WORKSPACES {
            window.click(SharedString::from(format!("rail-{}", w.route.as_str())), cx);
            window.render_frame(cx);
            assert_eq!(window.find("app-workspace").label(), Some(w.label));
            assert_eq!(shell.read(cx).route(), w.route);
            assert_eq!(
                window
                    .find(SharedString::from(format!("rail-{}", w.route.as_str())))
                    .selected(),
                Some(true)
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn imports_wait_for_the_ui_then_open_their_workspace(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    cx.update_window(handle, |_, window, cx| {
        shell.read(cx).focus_handle(cx).focus(window, cx)
    })
    .unwrap();
    dispatch(cx, handle, Import(ImportKindValue(ImportKind::Sermon)));
    assert!(
        requests(cx)
            .iter()
            .all(|r| !matches!(r, ShellRequest::Import(_))),
        "held until ready"
    );
    ready(cx, handle, &shell);
    assert!(requests(cx).contains(&ShellRequest::Import(ImportKind::Sermon)));
    cx.update(|cx| assert_eq!(shell.read(cx).route(), Route::Scripture));
    // A .kairo import has no route: the workspace stays.
    dispatch(cx, handle, Import(ImportKindValue(ImportKind::Kairo)));
    cx.update(|cx| assert_eq!(shell.read(cx).route(), Route::Scripture));
    assert_eq!(
        requests(cx).last(),
        Some(&ShellRequest::Import(ImportKind::Kairo))
    );
}

#[gpui_kit::test]
fn kairo_files_are_delivered_once_ready(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    let file = std::path::PathBuf::from("/tmp/service.kairo");
    cx.update(|cx| shell.update(cx, |s, cx| s.open_kairo_files(vec![file.clone()], cx)));
    assert!(requests(cx).is_empty());
    ready(cx, handle, &shell);
    assert_eq!(requests(cx), vec![ShellRequest::OpenKairoFiles(vec![file])]);
}

#[gpui_kit::test]
fn quitting_asks_once_and_cancel_allows_asking_again(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    dispatch(cx, handle, Quit);
    dispatch(cx, handle, Quit);
    cx.update_window(handle, |_, window, cx| {
        let dialogs = window.find_all_by_role(gpui_kit::Role::AlertDialog);
        assert_eq!(dialogs.len(), 1, "one prompt for repeated requests");
        assert_eq!(dialogs[0].label(), Some("Quit Kairo?"));
        window.click("keep-open", cx);
        window.render_frame(cx);
        assert!(
            window
                .find_all_by_role(gpui_kit::Role::AlertDialog)
                .is_empty()
        );
    })
    .unwrap();
    cx.update(|cx| assert!(!shell.read(cx).quit_open()));
    dispatch(cx, handle, Quit);
    cx.update(|cx| {
        assert!(
            shell.read(cx).quit_open(),
            "asked again after Keep Kairo open"
        )
    });
}

#[gpui_kit::test]
fn window_close_asks_and_confirming_lets_the_next_request_through(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    let allowed = cx
        .update_window(handle, |_, window, cx| {
            shell.update(cx, |s, cx| s.request_quit(window, cx))
        })
        .unwrap();
    assert!(!allowed, "the first close only asks");
    cx.update(|cx| {
        shell.update(cx, |s, cx| s.respond_to_quit(true, cx));
        assert!(!shell.read(cx).quit_open());
    });
}

#[gpui_kit::test]
fn the_updater_install_skips_the_question(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update(|cx| shell.update(cx, |s, _| s.allow_quit()));
    let allowed = cx
        .update_window(handle, |_, window, cx| {
            shell.update(cx, |s, cx| s.request_quit(window, cx))
        })
        .unwrap();
    assert!(allowed);
    cx.update(|cx| assert!(!shell.read(cx).quit_open()));
}

#[gpui_kit::test]
fn workspace_commands_follow_custom_bindings(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update_window(handle, |_, window, cx| {
        window.press("space", cx);
        window.press("pagedown", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            cx.global::<Seen>().commands,
            vec![ShortcutCommand::Approve, ShortcutCommand::Next]
        );
        cx.global_mut::<Seen>().commands.clear();
        let state = ShellState {
            shortcuts: ShortcutBindings {
                approve: Some(vec!["Mod+p".into()]),
                ..Default::default()
            },
            ..Default::default()
        };
        shell.update(cx, |s, cx| s.set_state(state, cx));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("space", cx);
        window.press("secondary-p", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(cx.global::<Seen>().commands, vec![ShortcutCommand::Approve]));
}

#[gpui_kit::test]
fn workspace_commands_never_fire_in_a_text_field(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update(|cx| {
        let state = ShellState {
            account: AccountView {
                signed_in: true,
                name: Some("Ada".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        shell.update(cx, |s, cx| s.set_state(state, cx));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("feedback", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let field = window.find_by_label("Your feedback");
        window.click(field.path().last().cloned().unwrap(), cx);
        window.input("Great", cx);
        window.press("space", cx);
        window.press("backspace", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(
            cx.global::<Seen>().commands.is_empty(),
            "{:?}",
            cx.global::<Seen>().commands
        )
    });
}

#[gpui_kit::test]
fn song_search_and_screens_shortcuts_open_and_escape_closes(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    dispatch(cx, handle, SongSearch);
    cx.update(|cx| assert_eq!(shell.read(cx).overlay(), Some(Overlay::SongSearch)));
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).overlay(), None));
    dispatch(cx, handle, ToggleScreens);
    cx.update(|cx| assert_eq!(shell.read(cx).overlay(), Some(Overlay::Screens)));
    dispatch(cx, handle, ToggleScreens);
    cx.update(|cx| assert_eq!(shell.read(cx).overlay(), None));
    // ⌥⌘1 is bound.
    cx.update_window(handle, |_, window, cx| {
        window.press("alt-secondary-1", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).overlay(), Some(Overlay::Screens)));
}

#[gpui_kit::test]
fn zoom_steps_the_interface_scale(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    dispatch(cx, handle, ZoomIn);
    dispatch(cx, handle, ZoomIn);
    cx.update(|cx| {
        assert_eq!(shell.read(cx).scale().factor(), 1.25);
        use gpui_kit::component::ActiveTheme;
        assert_eq!(cx.theme().font_size, px(20.));
    });
    dispatch(cx, handle, ZoomOut);
    dispatch(cx, handle, ActualSize);
    cx.update(|cx| assert_eq!(shell.read(cx).scale().factor(), 1.0));
}

#[gpui_kit::test]
fn whats_new_shows_after_an_update_but_waits_out_a_live_service(cx: &mut TestAppContext) {
    setup(cx);
    let entries = ui_shell::whats_new::changelog();
    let (newest, older) = (entries[0].version.clone(), entries[1].version.clone());
    let (handle, shell) = open(cx, &newest);
    ready(cx, handle, &shell);
    cx.update(|cx| {
        let live = ShellState {
            transcribing: true,
            whats_new_last_seen: Some(older.clone().into()),
            ..Default::default()
        };
        shell.update(cx, |s, cx| s.set_state(live, cx));
        assert!(shell.read(cx).whats_new().is_none(), "waits while live");
        let calm = ShellState {
            whats_new_last_seen: Some(older.into()),
            ..Default::default()
        };
        shell.update(cx, |s, cx| s.set_state(calm, cx));
        assert_eq!(
            shell.read(cx).whats_new().map(|e| e.version.clone()),
            Some(newest.clone())
        );
    });
    assert!(requests(cx).contains(&ShellRequest::MarkWhatsNewSeen(newest.into())));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find_all_by_role(gpui_kit::Role::Dialog)
                .iter()
                .any(|d| d.label() == Some("What's New in Kairo"))
        );
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert!(shell.read(cx).whats_new().is_none()));
}

#[gpui_kit::test]
fn a_fresh_install_records_the_version_without_a_sheet(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.33");
    ready(cx, handle, &shell);
    cx.update(|cx| {
        let state = ShellState {
            whats_new_last_seen: Some("".into()),
            ..Default::default()
        };
        shell.update(cx, |s, cx| s.set_state(state, cx));
        assert!(shell.read(cx).whats_new().is_none());
    });
    assert!(requests(cx).contains(&ShellRequest::MarkWhatsNewSeen("1.0.33".into())));
}

#[gpui_kit::test]
fn update_pill_takes_two_clicks_to_install(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("update-pill").is_none(),
            "nothing while idle"
        );
        shell.update(cx, |s, cx| {
            s.set_state(
                ShellState {
                    update: UpdateState::Downloaded {
                        version: "1.0.1".into(),
                    },
                    ..Default::default()
                },
                cx,
            )
        });
        window.render_frame(cx);
        window.click("update-pill", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!requests(cx).contains(&ShellRequest::InstallUpdate));
    cx.update_window(handle, |_, window, cx| {
        window.click("update-pill", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(requests(cx).contains(&ShellRequest::InstallUpdate));
}

#[gpui_kit::test]
fn clear_asks_the_app_to_clear_live_output(cx: &mut TestAppContext) {
    setup(cx);
    let (handle, shell) = open(cx, "1.0.0");
    ready(cx, handle, &shell);
    cx.update_window(handle, |_, window, cx| {
        window.click("clear", cx);
    })
    .unwrap();
    assert!(requests(cx).contains(&ShellRequest::ClearLive));
}
