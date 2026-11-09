//! Saved themes, built-in themes and library rules, from `src/lib/theme-library.ts`.
//!
//! TypeScript fills in `Date.now()` and a random id when they are omitted.
//! Here the caller passes both, so every function is pure.

use crate::js;
use crate::overlay::{
    CustomOverlayTheme, OverlayBackgroundType, OverlayContentKind, OverlayLayoutPosition,
    OverlayOutput, OverlayTextAlign, OverlayTheme, OverlayVerticalAlign, ReferencePosition,
};
use crate::overlay_boxes::apply_layout_preset;
use crate::overlay_defaults::{DEFAULT_OVERLAY_SETTINGS, normalize_overlay_theme};
use crate::overlay_outputs::{
    VariantPatch, is_rendered_kind, set_content_override, with_content_patch,
};
use crate::serde_util::Extra;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::LazyLock;

const UNTITLED: &str = "Untitled theme";

/// `createCustomTheme(name, theme, kind, now, id)`.
pub fn create_custom_theme(
    name: &str,
    theme: &OverlayTheme,
    kind: OverlayContentKind,
    now: f64,
    id: &str,
) -> CustomOverlayTheme {
    let name = js::trim(name);
    CustomOverlayTheme {
        id: id.to_owned(),
        name: if name.is_empty() {
            UNTITLED.into()
        } else {
            name.to_owned()
        },
        kind,
        created_at: now,
        updated_at: now,
        theme: theme.clone(),
        baseline: Some(theme.clone()),
        extra: Extra::new(),
    }
}

/// `nextUntitledThemeName`: "Untitled theme", then "Untitled theme 2", …
pub fn next_untitled_theme_name(
    library: &[CustomOverlayTheme],
    kind: OverlayContentKind,
) -> String {
    let names: HashSet<&str> = themes_for_kind(library, kind)
        .into_iter()
        .map(|item| item.name.as_str())
        .collect();
    if !names.contains(UNTITLED) {
        return UNTITLED.into();
    }
    let mut n = 2;
    while names.contains(format!("{UNTITLED} {n}").as_str()) {
        n += 1;
    }
    format!("{UNTITLED} {n}")
}

/// `themesForKind`.
pub fn themes_for_kind(
    library: &[CustomOverlayTheme],
    kind: OverlayContentKind,
) -> Vec<&CustomOverlayTheme> {
    library.iter().filter(|item| item.kind == kind).collect()
}

/// `updateLibraryTheme`: saves a draft onto one row. A row saved before
/// baselines existed takes its pre-edit theme as its baseline.
pub fn update_library_theme(
    library: &[CustomOverlayTheme],
    id: &str,
    name: &str,
    theme: &OverlayTheme,
    now: f64,
) -> Vec<CustomOverlayTheme> {
    library
        .iter()
        .map(|item| {
            if item.id != id {
                return item.clone();
            }
            CustomOverlayTheme {
                name: name.to_owned(),
                updated_at: now,
                theme: theme.clone(),
                baseline: Some(item.baseline.clone().unwrap_or_else(|| item.theme.clone())),
                ..item.clone()
            }
        })
        .collect()
}

/// `themeBaseline`: what Reset returns `item` to.
pub fn theme_baseline(item: &CustomOverlayTheme) -> &OverlayTheme {
    item.baseline.as_ref().unwrap_or(&item.theme)
}

/// `setThemeBaseline`: "Set as default".
pub fn set_theme_baseline(
    library: &[CustomOverlayTheme],
    id: &str,
    theme: &OverlayTheme,
) -> Vec<CustomOverlayTheme> {
    library
        .iter()
        .map(|item| {
            if item.id == id {
                CustomOverlayTheme {
                    baseline: Some(theme.clone()),
                    ..item.clone()
                }
            } else {
                item.clone()
            }
        })
        .collect()
}

/// `normalizeThemeLibrary(raw, activeTheme, now, legacyId)`. A non-array
/// migrates the active theme into "My current theme" with `legacy_id`.
/// Rows without a non-empty string id, and repeated ids, are dropped.
pub fn normalize_theme_library(
    raw: Option<&Value>,
    active_theme: &OverlayTheme,
    now: f64,
    legacy_id: &str,
) -> Vec<CustomOverlayTheme> {
    let Some(Value::Array(items)) = raw else {
        return vec![create_custom_theme(
            "My current theme",
            active_theme,
            OverlayContentKind::Scripture,
            now,
            legacy_id,
        )];
    };

    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for item in items {
        // `!item || typeof item !== 'object'` lets arrays through, but an
        // array has no `id`, so only objects can survive.
        let Value::Object(candidate) = item else {
            continue;
        };
        let Some(Value::String(id)) = candidate.get("id") else {
            continue;
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let number = |key: &str| candidate.get(key).and_then(Value::as_f64);
        let created_at = number("createdAt").unwrap_or(now);
        let name = match candidate.get("name") {
            Some(Value::String(name)) if !js::trim(name).is_empty() => js::trim(name).to_owned(),
            _ => UNTITLED.into(),
        };
        normalized.push(CustomOverlayTheme {
            id: id.clone(),
            name,
            // Everything saved before the split was a scripture theme.
            kind: if candidate.get("kind").and_then(Value::as_str) == Some("lyrics") {
                OverlayContentKind::Lyrics
            } else {
                OverlayContentKind::Scripture
            },
            created_at,
            updated_at: number("updatedAt").unwrap_or(created_at),
            theme: normalize_overlay_theme(candidate.get("theme")),
            baseline: js::truthy(candidate.get("baseline"))
                .then(|| normalize_overlay_theme(candidate.get("baseline"))),
            extra: Extra::new(),
        });
    }
    normalized
}

// ─── Themes on outputs ─────────────────────────────────────────────────────────

/// `outputUsesTheme`.
pub fn output_uses_theme(output: &OverlayOutput, id: &str, kind: OverlayContentKind) -> bool {
    match kind {
        OverlayContentKind::Lyrics => output
            .lyrics
            .as_ref()
            .is_some_and(|l| l.theme_id.as_deref() == Some(id)),
        OverlayContentKind::Scripture => output.theme_id.as_deref() == Some(id),
    }
}

/// `assignThemeToOutput`: a lyrics theme turns the lyrics override on.
pub fn assign_theme_to_output(output: &OverlayOutput, theme: &CustomOverlayTheme) -> OverlayOutput {
    if theme.kind == OverlayContentKind::Lyrics {
        let base = set_content_override(output, OverlayContentKind::Lyrics, true);
        return with_content_patch(
            &base,
            OverlayContentKind::Lyrics,
            VariantPatch {
                theme_id: Some(Some(theme.id.clone())),
                theme: Some(theme.theme.clone()),
                template: None,
            },
        );
    }
    OverlayOutput {
        theme_id: Some(theme.id.clone()),
        theme: theme.theme.clone(),
        ..output.clone()
    }
}

/// `unassignThemeFromOutput`: lyrics go back to following scripture.
pub fn unassign_theme_from_output(
    output: &OverlayOutput,
    id: &str,
    kind: OverlayContentKind,
) -> OverlayOutput {
    if kind != OverlayContentKind::Lyrics || !output_uses_theme(output, id, kind) {
        return output.clone();
    }
    set_content_override(output, OverlayContentKind::Lyrics, false)
}

/// `syncThemeToOutputs`: refreshes every output's copy after an edit.
pub fn sync_theme_to_outputs(
    outputs: &[OverlayOutput],
    theme: &CustomOverlayTheme,
) -> Vec<OverlayOutput> {
    outputs
        .iter()
        .map(|output| {
            if output_uses_theme(output, &theme.id, theme.kind) {
                assign_theme_to_output(output, theme)
            } else {
                output.clone()
            }
        })
        .collect()
}

/// `detachThemeFromOutputs`: after a delete, outputs keep their look unlinked.
pub fn detach_theme_from_outputs(
    outputs: &[OverlayOutput],
    id: &str,
    kind: OverlayContentKind,
) -> Vec<OverlayOutput> {
    outputs
        .iter()
        .map(|output| {
            let mut next = output.clone();
            if output_uses_theme(output, id, kind) {
                match (kind, next.lyrics.as_mut()) {
                    (OverlayContentKind::Lyrics, Some(lyrics)) => lyrics.theme_id = None,
                    _ => next.theme_id = None,
                }
            }
            next
        })
        .collect()
}

// ─── Built-in themes ──────────────────────────────────────────────────────────

/// One `BUILT_IN_THEMES` entry.
#[derive(Clone, Debug, PartialEq)]
pub struct BuiltInTheme {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: OverlayContentKind,
    pub theme: OverlayTheme,
}

/// `BUILT_IN_THEMES`.
pub static BUILT_IN_THEMES: LazyLock<Vec<BuiltInTheme>> = LazyLock::new(|| {
    let d = &DEFAULT_OVERLAY_SETTINGS.theme;

    let mut warm = d.clone();
    warm.background.kind = OverlayBackgroundType::Color;
    warm.background.color = "#e8dfcf".into();
    warm.verse.font_family = "Georgia, 'Times New Roman', serif".into();
    warm.verse.color = "#201d19".into();
    warm.verse.align = OverlayTextAlign::Left;
    warm.verse.shadow.enabled = false;
    warm.reference.text.color = "#8b4b32".into();
    warm.reference.position = ReferencePosition::Above;
    warm.layout.backdrop_box = false;

    let mut midnight = d.clone();
    midnight.background.kind = OverlayBackgroundType::Gradient;
    midnight.background.color = "#07111f".into();
    midnight.background.color2 = Some("#18324b".into());
    midnight.background.angle_deg = Some(135.0);
    midnight.reference.text.color = "#AABED7".into();
    midnight.layout.backdrop_box = false;

    let mut stage = d.clone();
    stage.background.kind = OverlayBackgroundType::Transparent;
    stage.verse.font_size_px = 96.0;
    stage.verse.font_weight = 700.0;
    stage.verse.align = OverlayTextAlign::Center;
    stage.verse.vertical_align = OverlayVerticalAlign::Middle;
    stage.verse.line_height = 1.25;
    stage.reference.show = false;
    stage.layout.backdrop_box = false;
    stage.layout.auto_fit_text = true;

    let mut lower = d.clone();
    lower.background.kind = OverlayBackgroundType::Transparent;
    lower.verse.font_size_px = 64.0;
    lower.verse.font_weight = 600.0;
    lower.verse.align = OverlayTextAlign::Center;
    lower.verse.vertical_align = OverlayVerticalAlign::Bottom;
    lower.reference.show = false;
    lower.layout.auto_fit_text = true;

    use OverlayContentKind::{Lyrics, Scripture};
    use OverlayLayoutPosition::{Center, Full, LowerThird};
    vec![
        BuiltInTheme {
            id: "broadcast",
            name: "Broadcast",
            kind: Scripture,
            theme: d.clone(),
        },
        BuiltInTheme {
            id: "warm-paper",
            name: "Warm paper",
            kind: Scripture,
            theme: apply_layout_preset(&warm, Center, Some(72.0)),
        },
        BuiltInTheme {
            id: "midnight",
            name: "Midnight",
            kind: Scripture,
            theme: apply_layout_preset(&midnight, Center, Some(76.0)),
        },
        BuiltInTheme {
            id: "lyrics-stage",
            name: "Stage lyrics",
            kind: Lyrics,
            theme: apply_layout_preset(&stage, Full, Some(92.0)),
        },
        BuiltInTheme {
            id: "lyrics-lower",
            name: "Lyrics lower third",
            kind: Lyrics,
            theme: apply_layout_preset(&lower, LowerThird, Some(84.0)),
        },
    ]
});

/// `builtInThemesForKind`.
pub fn built_in_themes_for_kind(kind: OverlayContentKind) -> Vec<&'static BuiltInTheme> {
    BUILT_IN_THEMES
        .iter()
        .filter(|item| item.kind == kind)
        .collect()
}

/// `isUntouchedThemeLibrary`: empty, or only the unchanged first-launch
/// "My current theme" (including the older backdrop-on default).
pub fn is_untouched_theme_library(library: &[CustomOverlayTheme]) -> bool {
    match library {
        [] => true,
        [only] => {
            if only.kind != OverlayContentKind::Scripture || only.name != "My current theme" {
                return false;
            }
            let stored = normalize_typed(&only.theme);
            let current = normalize_typed(&DEFAULT_OVERLAY_SETTINGS.theme);
            let mut legacy = current.clone();
            legacy.layout.backdrop_box = true;
            // The TypeScript compares JSON text; normalised themes have a fixed
            // key order and no NaN, so value equality is the same test.
            stored == current || stored == legacy
        }
        _ => false,
    }
}

fn normalize_typed(theme: &OverlayTheme) -> OverlayTheme {
    let value = crate::json::to_js_value(theme).expect("themes always serialise");
    normalize_overlay_theme(Some(&value))
}

/// `seedBuiltInThemes(library, now)`: every built-in as a library theme.
/// The first reuses the untouched default's id so screens pointing at it
/// still do; the others take ids from `new_id`, in order.
pub fn seed_built_in_themes(
    library: &[CustomOverlayTheme],
    now: f64,
    mut new_id: impl FnMut() -> String,
) -> Vec<CustomOverlayTheme> {
    let reuse_id = library
        .first()
        .map(|item| item.id.as_str())
        .filter(|id| !id.is_empty());
    BUILT_IN_THEMES
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let id = match reuse_id {
                Some(id) if index == 0 => id.to_owned(),
                _ => new_id(),
            };
            create_custom_theme(item.name, &item.theme, item.kind, now + index as f64, &id)
        })
        .collect()
}

/// The `themeDefaults` setting: a theme id per content kind.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeDefaults {
    pub scripture: Option<String>,
    pub lyrics: Option<String>,
}

/// `withDefaultThemes`: a new output takes the default theme of each kind
/// that has one still in the library.
pub fn with_default_themes(
    output: &OverlayOutput,
    library: &[CustomOverlayTheme],
    defaults: Option<&ThemeDefaults>,
) -> OverlayOutput {
    if !is_rendered_kind(output.kind) {
        return output.clone();
    }
    let mut next = output.clone();
    for kind in [OverlayContentKind::Scripture, OverlayContentKind::Lyrics] {
        let wanted = defaults.and_then(|d| match kind {
            OverlayContentKind::Scripture => d.scripture.as_deref(),
            OverlayContentKind::Lyrics => d.lyrics.as_deref(),
        });
        // `entry.id === defaults?.[kind]`: an unset default matches no entry.
        if let Some(theme) = library
            .iter()
            .find(|entry| Some(entry.id.as_str()) == wanted && entry.kind == kind)
        {
            next = assign_theme_to_output(&next, theme);
        }
    }
    next
}
