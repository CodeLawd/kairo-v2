//! The native menus (`setupApplicationMenu` in `src/main/index.ts`).
//!
//! Dropped, because they only make sense in a browser: View → Reload,
//! Force Reload and Toggle Developer Tools (DECISIONS.md, 071). View → Zoom
//! In, Zoom Out and Actual Size drive the persisted interface scale.

use crate::keymap::{
    About, ActualSize, BringAllToFront, CheckForUpdates, CloseWindow, Hide, HideOthers, Import,
    ImportKindValue, Minimize, Quit, ShowAll, ToggleFullScreen, WhatsNew, ZoomIn, ZoomOut,
    ZoomWindow,
};
use crate::route::Route;
use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::{Menu, MenuItem, OsAction, SystemMenuType};
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

/// The menu bar for this platform.
pub fn app_menus(mac: bool) -> Vec<Menu> {
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
    menus.push(Menu {
        name: "View".into(),
        items: vec![
            MenuItem::action("Actual Size", ActualSize),
            MenuItem::action("Zoom In", ZoomIn),
            MenuItem::action("Zoom Out", ZoomOut),
            MenuItem::separator(),
            MenuItem::action("Toggle Full Screen", ToggleFullScreen),
        ],
        disabled: false,
    });
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
        let menus = app_menus(true);
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
        assert_eq!(titles, ["Kairo", "File", "Edit", "View", "Window"]);
        assert_eq!(names(&menus[1]), ["Import>", "-", "Close Window"]);
        assert_eq!(
            names(&menus[3]),
            [
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
        let menus = app_menus(false);
        let titles: Vec<_> = menus.iter().map(|m| m.name.to_string()).collect();
        assert_eq!(titles, ["File", "Edit", "View", "Window", "Help"]);
        assert_eq!(names(&menus[0]).last().map(String::as_str), Some("Exit"));
        assert_eq!(names(&menus[4]), ["What's New in Kairo"]);
    }

    #[test]
    fn no_browser_only_items() {
        for menu in app_menus(true).iter().chain(app_menus(false).iter()) {
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
