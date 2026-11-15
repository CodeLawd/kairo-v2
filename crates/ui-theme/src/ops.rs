//! The edits the editor makes to the stored settings. One function applies
//! each to [`AppSettings`], so the editor (optimistically) and the
//! application's writer (against the stored copy) end in the same state.
//!
//! Electron did these as separate writes: `themeLibrary` for the library,
//! `themeDefaults` for defaults, and `saveOutputSettings` for the screens
//! that use a theme. Here one [`Op`] is one atomic read-modify-write.

use kairo_domain::output_settings::OutputSettingsWriter;
use kairo_domain::overlay::{CustomOverlayTheme, OverlayOutput, OverlayTheme};
use kairo_domain::overlay_outputs::is_rendered_kind;
use kairo_domain::settings::AppSettings;
use kairo_domain::theme_library::{
    assign_theme_to_output, detach_theme_from_outputs, output_uses_theme, set_theme_baseline,
    sync_theme_to_outputs, unassign_theme_from_output, update_library_theme,
};

#[allow(clippy::large_enum_variant)]
/// One change to the library, the defaults or the screens' themes.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// The draft, saved onto its row and followed by every screen using it.
    Save {
        id: String,
        name: String,
        theme: OverlayTheme,
        now: f64,
    },
    /// A new row at the end of the library.
    Add(CustomOverlayTheme),
    /// "Delete": the row goes; screens keep their look, unlinked.
    Remove { id: String },
    /// "Save theme": what Reset returns to.
    SetBaseline { id: String, theme: OverlayTheme },
    /// The status bar's screen chips.
    ToggleOutput { theme_id: String, output_id: String },
    /// This theme becomes the default for its kind, and every rendered
    /// screen switches to it.
    MakeDefault { id: String },
}

/// Applies `op`. Unknown ids change nothing.
pub fn apply(settings: &mut AppSettings, op: &Op) {
    match op {
        Op::Save {
            id,
            name,
            theme,
            now,
        } => {
            settings.theme_library =
                update_library_theme(&settings.theme_library, id, name, theme, *now);
            if let Some(entry) = settings.theme_library.iter().find(|t| t.id == *id).cloned()
                && settings
                    .overlay
                    .outputs
                    .iter()
                    .any(|o| output_uses_theme(o, &entry.id, entry.kind))
            {
                edit_outputs(settings, |outputs| sync_theme_to_outputs(&outputs, &entry));
            }
        }
        Op::Add(item) => settings.theme_library.push(item.clone()),
        Op::Remove { id } => {
            let Some(entry) = settings.theme_library.iter().find(|t| t.id == *id).cloned() else {
                return;
            };
            settings.theme_library.retain(|t| t.id != *id);
            if settings
                .overlay
                .outputs
                .iter()
                .any(|o| output_uses_theme(o, &entry.id, entry.kind))
            {
                edit_outputs(settings, |outputs| {
                    detach_theme_from_outputs(&outputs, &entry.id, entry.kind)
                });
            }
        }
        Op::SetBaseline { id, theme } => {
            settings.theme_library = set_theme_baseline(&settings.theme_library, id, theme);
        }
        Op::ToggleOutput {
            theme_id,
            output_id,
        } => {
            let Some(entry) = settings
                .theme_library
                .iter()
                .find(|t| t.id == *theme_id)
                .cloned()
            else {
                return;
            };
            edit_outputs(settings, |outputs| {
                outputs
                    .iter()
                    .map(|o| {
                        if o.id != *output_id {
                            o.clone()
                        } else if output_uses_theme(o, &entry.id, entry.kind) {
                            unassign_theme_from_output(o, &entry.id, entry.kind)
                        } else {
                            assign_theme_to_output(o, &entry)
                        }
                    })
                    .collect()
            });
        }
        Op::MakeDefault { id } => {
            let Some(entry) = settings.theme_library.iter().find(|t| t.id == *id).cloned() else {
                return;
            };
            match entry.kind {
                kairo_domain::overlay::OverlayContentKind::Scripture => {
                    settings.theme_defaults.scripture = Some(entry.id.clone());
                }
                kairo_domain::overlay::OverlayContentKind::Lyrics => {
                    settings.theme_defaults.lyrics = Some(entry.id.clone());
                }
            }
            edit_outputs(settings, |outputs| {
                outputs
                    .iter()
                    .map(|o| {
                        if is_rendered_kind(o.kind) {
                            assign_theme_to_output(o, &entry)
                        } else {
                            o.clone()
                        }
                    })
                    .collect()
            });
        }
    }
}

/// `saveOutputSettings`: the new outputs are normalised, and the deprecated
/// `overlay.theme` follows the live scripture theme.
fn edit_outputs(
    settings: &mut AppSettings,
    edit: impl FnOnce(Vec<OverlayOutput>) -> Vec<OverlayOutput>,
) {
    let write = OutputSettingsWriter::new().begin(settings.overlay.clone(), edit);
    settings.overlay.outputs = write.next.outputs;
    settings.overlay.theme = write.next.theme;
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay::OverlayContentKind;
    use kairo_domain::overlay_defaults::{DEFAULT_OVERLAY_THEME, make_overlay_output};
    use kairo_domain::theme_library::create_custom_theme;

    fn settings() -> AppSettings {
        let mut s =
            AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
        let a = create_custom_theme(
            "A",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Scripture,
            1.0,
            "a",
        );
        let b = create_custom_theme(
            "B",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Scripture,
            1.0,
            "b",
        );
        let l = create_custom_theme(
            "L",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Lyrics,
            1.0,
            "l",
        );
        s.theme_library = vec![a.clone(), b, l];
        let mut screen =
            make_overlay_output("screen-1", kairo_domain::overlay::OverlayOutputKind::Screen);
        screen.enabled = true;
        screen = assign_theme_to_output(&screen, &a);
        s.overlay.outputs = vec![screen];
        s
    }

    fn edited(size: f64) -> OverlayTheme {
        let mut t = DEFAULT_OVERLAY_THEME.clone();
        t.verse.font_size_px = size;
        t
    }

    #[test]
    fn save_updates_the_row_and_the_screens_using_it() {
        let mut s = settings();
        apply(
            &mut s,
            &Op::Save {
                id: "a".into(),
                name: "  Renamed ".into(),
                theme: edited(77.0),
                now: 9.0,
            },
        );
        let a = s.theme_library.iter().find(|t| t.id == "a").unwrap();
        assert_eq!(a.theme.verse.font_size_px, 77.0);
        assert_eq!(a.updated_at, 9.0);
        assert_eq!(s.overlay.outputs[0].theme.verse.font_size_px, 77.0);
        // The deprecated theme follows the live scripture theme.
        assert_eq!(s.overlay.theme.verse.font_size_px, 77.0);
    }

    #[test]
    fn save_leaves_screens_on_other_themes_alone() {
        let mut s = settings();
        let before = s.overlay.outputs.clone();
        apply(
            &mut s,
            &Op::Save {
                id: "b".into(),
                name: "B".into(),
                theme: edited(55.0),
                now: 2.0,
            },
        );
        assert_eq!(s.overlay.outputs, before);
    }

    #[test]
    fn remove_unlinks_screens_but_keeps_their_look() {
        let mut s = settings();
        apply(&mut s, &Op::Remove { id: "a".into() });
        assert!(s.theme_library.iter().all(|t| t.id != "a"));
        assert_eq!(s.overlay.outputs[0].theme_id, None);
    }

    #[test]
    fn toggle_assigns_then_a_lyrics_theme_unassigns() {
        let mut s = settings();
        apply(
            &mut s,
            &Op::ToggleOutput {
                theme_id: "l".into(),
                output_id: "screen-1".into(),
            },
        );
        assert!(output_uses_theme(
            &s.overlay.outputs[0],
            "l",
            OverlayContentKind::Lyrics
        ));
        apply(
            &mut s,
            &Op::ToggleOutput {
                theme_id: "l".into(),
                output_id: "screen-1".into(),
            },
        );
        assert!(!output_uses_theme(
            &s.overlay.outputs[0],
            "l",
            OverlayContentKind::Lyrics
        ));
    }

    #[test]
    fn make_default_sets_the_kind_and_moves_every_rendered_screen() {
        let mut s = settings();
        apply(&mut s, &Op::MakeDefault { id: "b".into() });
        assert_eq!(s.theme_defaults.scripture.as_deref(), Some("b"));
        assert_eq!(s.theme_defaults.lyrics, None);
        assert_eq!(s.overlay.outputs[0].theme_id.as_deref(), Some("b"));
    }

    #[test]
    fn unknown_ids_change_nothing() {
        let mut s = settings();
        let before = s.clone();
        apply(&mut s, &Op::MakeDefault { id: "nope".into() });
        apply(&mut s, &Op::Remove { id: "nope".into() });
        apply(
            &mut s,
            &Op::ToggleOutput {
                theme_id: "nope".into(),
                output_id: "screen-1".into(),
            },
        );
        assert_eq!(s.theme_library, before.theme_library);
        assert_eq!(s.overlay.outputs, before.overlay.outputs);
    }

    #[test]
    fn baseline_is_set_on_one_row() {
        let mut s = settings();
        apply(
            &mut s,
            &Op::SetBaseline {
                id: "b".into(),
                theme: edited(12.0),
            },
        );
        assert_eq!(
            s.theme_library[1]
                .baseline
                .as_ref()
                .unwrap()
                .verse
                .font_size_px,
            12.0
        );
        assert_ne!(
            s.theme_library[0]
                .baseline
                .as_ref()
                .unwrap()
                .verse
                .font_size_px,
            12.0
        );
    }
}
