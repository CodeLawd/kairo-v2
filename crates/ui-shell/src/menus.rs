//! The native menus (`setupApplicationMenu` in `src/main/index.ts`).
//!
//! Dropped, because they only make sense in a browser: View → Reload,
//! Force Reload and Toggle Developer Tools (DECISIONS.md, 071). View → Zoom
//! In, Zoom Out and Actual Size drive the persisted interface scale.

use crate::keymap::{
    About, ActualSize, BringAllToFront, CheckForUpdates, CloseWindow, Hide, HideOthers, Import,
    ImportKindValue, Minimize, OpenCaptureSettings, OpenScreenThemes, OpenWorkspace, Quit,
    RouteValue, ShowAll, SongSearch, ToggleAudienceScreens, ToggleCapture, ToggleFullScreen,
    ToggleMediaBin, ToggleScreens, ToggleStageScreens, WhatsNew, ZoomIn, ZoomOut, ZoomWindow,
};
use crate::route::{Route, WORKSPACES};
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{Menu, MenuItem, NoAction, OsAction, SystemMenuType};
use kairo_domain::import_menu::IMPORT_OPTIONS;

/// The product name in menus.
pub const APP_NAME: &str = "Kairo";

/// File → Import: one item per option, with a separator between groups.
/// Kairo files have no route and sit in a group by themselves.
pub fn import_items() -> Vec<MenuItem> {
    let mut items = Vec::new();
    let mut previous: Option<&str> = None;
    for (kind, label, route) in IMPORT_OPTIONS {
        let group = route.unwrap_or(kind.as_str());
        if previous.is_some_and(|p| p != group) {
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::action(*label, Import(ImportKindValue(*kind))));
        previous = Some(group);
    }
    items
}

/// What the Screens menu shows: its labels follow the screens' state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreensMenu {
    /// Any Audience screen is on; `None` when none is set up.
    pub audience_on: Option<bool>,
    /// Any Stage screen is on; `None` when none is set up.
    pub stage_on: Option<bool>,
    /// A capture is recording.
    pub recording: bool,
}

/// A greyed group title, as the system draws a section heading.
fn heading(name: &'static str) -> MenuItem {
    MenuItem::action(name, NoAction).disabled(true)
}

/// Turn a screen group on or off, or set one up when there is none.
fn group_switch(on: Option<bool>, action: impl gpui_kit::Action) -> MenuItem {
    MenuItem::action(
        match on {
            Some(true) => "Turn Off",
            Some(false) => "Turn On",
            None => "Set Up…",
        },
        action,
    )
}

/// Screens (after ProPresenter's): the screen groups, setup and capture.
pub fn screens_menu(state: ScreensMenu) -> Menu {
    Menu {
        name: "Screens".into(),
        items: vec![
            heading("Audience"),
            group_switch(state.audience_on, ToggleAudienceScreens),
            MenuItem::action("Looks…", OpenScreenThemes),
            MenuItem::separator(),
            heading("Stage"),
            group_switch(state.stage_on, ToggleStageScreens),
            MenuItem::separator(),
            MenuItem::action("Configure Screens…", ToggleScreens),
            MenuItem::separator(),
            MenuItem::action(
                if state.recording {
                    "Stop Capture"
                } else {
                    "Start Capture"
                },
                ToggleCapture,
            ),
            MenuItem::action("Capture Settings…", OpenCaptureSettings),
        ],
        disabled: false,
    }
}

/// The menu bar for this platform.
/// View (after ProPresenter's): every workspace with its shortcut and a
/// check on the open one, then the media dock, Looks and search, then zoom.
pub fn view_menu(route: Route) -> Menu {
    let mut items: Vec<MenuItem> = WORKSPACES
        .iter()
        .map(|w| {
            MenuItem::action(w.label, OpenWorkspace(RouteValue(w.route))).checked(w.route == route)
        })
        .collect();
    items.extend([
        MenuItem::separator(),
        MenuItem::action("Media Bin", ToggleMediaBin),
        MenuItem::action("Looks…", OpenScreenThemes),
        MenuItem::action("Search…", SongSearch),
        MenuItem::separator(),
        MenuItem::action("Actual Size", ActualSize),
        MenuItem::action("Zoom In", ZoomIn),
        MenuItem::action("Zoom Out", ZoomOut),
        MenuItem::separator(),
        MenuItem::action("Toggle Full Screen", ToggleFullScreen),
    ]);
    Menu {
        name: "View".into(),
        items,
        disabled: false,
    }
}

pub fn app_menus(mac: bool, screens: ScreensMenu, route: Route) -> Vec<Menu> {
    let mut menus = Vec::new();
    if mac {
        menus.push(Menu {
            name: APP_NAME.into(),
            items: vec![
                MenuItem::action(format!("About {APP_NAME}"), About),
                MenuItem::action("Check for Updates…", CheckForUpdates),
                MenuItem::action(format!("What's New in {APP_NAME}"), WhatsNew),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action(format!("Hide {APP_NAME}"), Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action(format!("Quit {APP_NAME}"), Quit),
            ],
            disabled: false,
        });
    }
    menus.push(Menu {
        name: "File".into(),
        items: vec![
            MenuItem::submenu(Menu {
                name: "Import".into(),
                items: import_items(),
                disabled: false,
            }),
            MenuItem::separator(),
            if mac {
                MenuItem::action("Close Window", CloseWindow)
            } else {
                MenuItem::action("Exit", Quit)
            },
        ],
        disabled: false,
    });
    menus.push(Menu {
        name: "Edit".into(),
        items: vec![
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Cut", Cut, OsAction::Cut),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
        ],
        disabled: false,
    });
    menus.push(view_menu(route));
    menus.push(screens_menu(screens));
    let mut window = vec![
        MenuItem::action("Minimize", Minimize),
        MenuItem::action("Zoom", ZoomWindow),
    ];
    if mac {
        window.push(MenuItem::separator());
        window.push(MenuItem::action("Bring All to Front", BringAllToFront));
    } else {
        window.push(MenuItem::action("Close", CloseWindow));
    }
    menus.push(Menu {
        name: "Window".into(),
        items: window,
        disabled: false,
    });
    if !mac {
        menus.push(Menu {
            name: "Help".into(),
            items: vec![MenuItem::action(
                format!("What's New in {APP_NAME}"),
                WhatsNew,
            )],
            disabled: false,
        });
    }
    menus
}

/// The route an Import item opens, for tests and the shell.
pub fn import_route_for(kind: kairo_domain::import_menu::ImportKind) -> Option<Route> {
    crate::route::import_route(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(menu: &Menu) -> Vec<String> {
        menu.items
            .iter()
            .map(|item| match item {
                MenuItem::Separator => "-".into(),
                MenuItem::Submenu(menu) => format!("{}>", menu.name),
                MenuItem::SystemMenu(menu) => format!("[{}]", menu.name),
                MenuItem::Action { name, .. } => name.to_string(),
            })
            .collect()
    }

    #[test]
    fn mac_app_menu_matches_the_roles() {
        let menus = app_menus(true, ScreensMenu::default(), Route::Operator);
        assert_eq!(
            names(&menus[0]),
            [
                "About Kairo",
                "Check for Updates…",
                "What's New in Kairo",
                "-",
                "[Services]",
                "-",
                "Hide Kairo",
                "Hide Others",
                "Show All",
                "-",
                "Quit Kairo",
            ]
        );
        let titles: Vec<_> = menus.iter().map(|m| m.name.to_string()).collect();
        assert_eq!(
            titles,
            ["Kairo", "File", "Edit", "View", "Screens", "Window"]
        );
        assert_eq!(names(&menus[1]), ["Import>", "-", "Close Window"]);
        assert_eq!(
            names(&menus[3]),
            [
                "Operator",
                "Bible",
                "Lyrics",
                "Slides",
                "Imagine",
                "Engage",
                "Web",
                "Prompter",
                "Theme Editor",
                "-",
                "Media Bin",
                "Looks…",
                "Search…",
                "-",
                "Actual Size",
                "Zoom In",
                "Zoom Out",
                "-",
                "Toggle Full Screen"
            ]
        );
    }

    #[test]
    fn other_platforms_quit_from_file_and_have_help() {
        let menus = app_menus(false, ScreensMenu::default(), Route::Operator);
        let titles: Vec<_> = menus.iter().map(|m| m.name.to_string()).collect();
        assert_eq!(
            titles,
            ["File", "Edit", "View", "Screens", "Window", "Help"]
        );
        assert_eq!(names(&menus[0]).last().map(String::as_str), Some("Exit"));
        assert_eq!(names(&menus[5]), ["What's New in Kairo"]);
    }

    #[test]
    fn view_checks_the_open_workspace() {
        let view = view_menu(Route::Lyrics);
        let checked: Vec<String> = view
            .items
            .iter()
            .filter(|i| i.is_checked())
            .map(|i| match i {
                MenuItem::Action { name, .. } => name.to_string(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(checked, ["Lyrics"]);
        assert!(crate::keymap::workspace_key(0).is_some());
        assert!(
            crate::keymap::workspace_key(9).is_none(),
            "nine keys at most"
        );
    }

    #[test]
    fn screens_menu_follows_the_screens() {
        let off = screens_menu(ScreensMenu {
            audience_on: Some(true),
            stage_on: None,
            recording: false,
        });
        assert_eq!(
            names(&off),
            [
                "Audience",
                "Turn Off",
                "Looks…",
                "-",
                "Stage",
                "Set Up…",
                "-",
                "Configure Screens…",
                "-",
                "Start Capture",
                "Capture Settings…",
            ]
        );
        let on = screens_menu(ScreensMenu {
            audience_on: Some(false),
            stage_on: Some(true),
            recording: true,
        });
        let on = names(&on);
        assert_eq!(on[1], "Turn On");
        assert_eq!(on[5], "Turn Off");
        assert_eq!(on[9], "Stop Capture");
    }

    #[test]
    fn no_browser_only_items() {
        for menu in app_menus(true, ScreensMenu::default(), Route::Operator)
            .iter()
            .chain(app_menus(false, ScreensMenu::default(), Route::Operator).iter())
        {
            for name in names(menu) {
                assert!(
                    !name.contains("Reload") && !name.contains("Developer"),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn import_groups_are_separated_by_route() {
        let items = import_items();
        let labels: Vec<_> = items
            .iter()
            .map(|i| match i {
                MenuItem::Separator => "-".to_owned(),
                MenuItem::Action { name, .. } => name.to_string(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(
            labels,
            [
                "PDF document…",
                "PowerPoint presentation…",
                "-",
                "Images…",
                "Videos…",
                "Audio files…",
                "-",
                "Sermon notes…",
                "-",
                "Lyrics file…",
                "-",
                "Kairo file (.kairo)…",
            ]
        );
    }
}
