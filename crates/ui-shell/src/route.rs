//! The workspaces (`NavRoute` in `App.tsx`) and the rail's list of them
//! (`workspaces` in `AppShell.tsx`).

use kairo_domain::import_menu::{IMPORT_OPTIONS, ImportKind};
use kairo_ui_kit::{Hue, KairoIcon};

/// A workspace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Route {
    /// The operator booth. The app opens here.
    #[default]
    Operator,
    /// Bible.
    Scripture,
    /// Lyrics.
    Lyrics,
    /// Slides.
    Documents,
    /// Engage.
    Audience,
    /// Web.
    Web,
    /// Theme Editor.
    Theme,
}

/// One rail entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workspace {
    /// The route.
    pub route: Route,
    /// Full name, also the accessible name.
    pub label: &'static str,
    /// A shorter name for the narrow rail.
    pub rail: Option<&'static str>,
    /// What the shortened name stands for, on hover.
    pub hint: Option<&'static str>,
    /// The mark.
    pub icon: KairoIcon,
    /// The tool colour.
    pub hue: Hue,
}

impl Workspace {
    /// What the rail shows under the mark.
    pub fn rail_label(&self) -> &'static str {
        self.rail.unwrap_or(self.label)
    }

    /// The tooltip: only a shortened rail label needs the full name.
    pub fn tooltip(&self) -> Option<String> {
        let _ = self.rail?;
        Some(match self.hint {
            Some(hint) => format!("{} — {hint}", self.label),
            None => self.label.to_owned(),
        })
    }
}

/// The rail, in order.
pub const WORKSPACES: [Workspace; 7] = [
    Workspace {
        route: Route::Operator,
        label: "Operator",
        rail: None,
        hint: None,
        icon: KairoIcon::PhGauge,
        hue: Hue::Green,
    },
    Workspace {
        route: Route::Scripture,
        label: "Bible",
        rail: None,
        hint: None,
        icon: KairoIcon::Bible,
        hue: Hue::Blue,
    },
    Workspace {
        route: Route::Lyrics,
        label: "Lyrics",
        rail: None,
        hint: None,
        icon: KairoIcon::PhMusicNotes,
        hue: Hue::Violet,
    },
    Workspace {
        route: Route::Documents,
        label: "Slides",
        rail: None,
        hint: None,
        icon: KairoIcon::PhPresentation,
        hue: Hue::Cyan,
    },
    Workspace {
        route: Route::Audience,
        label: "Engage",
        rail: None,
        hint: None,
        icon: KairoIcon::PhChatsCircle,
        hue: Hue::Coral,
    },
    Workspace {
        route: Route::Web,
        label: "Web",
        rail: None,
        hint: None,
        icon: KairoIcon::PhGlobe,
        hue: Hue::Indigo,
    },
    Workspace {
        route: Route::Theme,
        label: "Theme Editor",
        rail: Some("Theme"),
        hint: Some("Create and edit themes"),
        icon: KairoIcon::PhPalette,
        hue: Hue::Pink,
    },
];

impl Route {
    /// The `NavRoute` string.
    pub fn as_str(self) -> &'static str {
        match self {
            Route::Operator => "operator",
            Route::Scripture => "scripture",
            Route::Lyrics => "lyrics",
            Route::Documents => "documents",
            Route::Audience => "audience",
            Route::Web => "web",
            Route::Theme => "theme",
        }
    }

    /// Parses a `NavRoute` string.
    pub fn from_name(name: &str) -> Option<Self> {
        WORKSPACES
            .iter()
            .map(|w| w.route)
            .find(|r| r.as_str() == name)
    }

    /// The rail entry.
    pub fn workspace(self) -> &'static Workspace {
        WORKSPACES
            .iter()
            .find(|w| w.route == self)
            .expect("every route is on the rail")
    }

    /// The prompt that builds this workspace's content.
    pub fn owner_prompt(self) -> &'static str {
        match self {
            Route::Operator => "076",
            Route::Scripture => "078",
            Route::Lyrics => "080",
            Route::Documents => "085",
            Route::Audience => "090",
            Route::Web => "089",
            Route::Theme => "086",
        }
    }

    /// Kept mounted across navigation so live state, the open passage, the
    /// theme draft and a page's place survive tab switches.
    pub fn stays_mounted(self) -> bool {
        matches!(
            self,
            Route::Operator | Route::Scripture | Route::Theme | Route::Documents | Route::Web
        )
    }
}

/// The route an Import menu item opens, if any (`IMPORT_OPTIONS`).
pub fn import_route(kind: ImportKind) -> Option<Route> {
    IMPORT_OPTIONS
        .iter()
        .find(|(k, _, _)| *k == kind)
        .and_then(|(_, _, route)| route.and_then(Route::from_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_round_trip_and_operator_is_first() {
        assert_eq!(WORKSPACES[0].route, Route::default());
        for w in WORKSPACES {
            assert_eq!(Route::from_name(w.route.as_str()), Some(w.route));
        }
        assert_eq!(Route::from_name("settings"), None);
    }

    #[test]
    fn only_theme_has_a_short_rail_label() {
        assert_eq!(Route::Theme.workspace().rail_label(), "Theme");
        assert_eq!(
            Route::Theme.workspace().tooltip().as_deref(),
            Some("Theme Editor — Create and edit themes")
        );
        assert_eq!(Route::Lyrics.workspace().tooltip(), None);
    }

    #[test]
    fn imports_open_their_workspace() {
        assert_eq!(import_route(ImportKind::Pdf), Some(Route::Documents));
        assert_eq!(import_route(ImportKind::Sermon), Some(Route::Scripture));
        assert_eq!(import_route(ImportKind::Audio), Some(Route::Operator));
        assert_eq!(import_route(ImportKind::Kairo), None);
    }
}
