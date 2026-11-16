//! Scripture: the default Bible, the Bible library
//! (`LocalBiblePackManager.tsx`), auto-detection and verse timing
//! (`ScriptureLatencyPanel.tsx`).

use crate::model::{
    PackBusy, PackState, Section, TraceFilter, headline, library, ms, needs_download,
    resolver_label, stages, status_label, trace_summary, translation_choices,
};
use crate::widgets;
use crate::window::SettingsWindow;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, ParentElement, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px,
    relative,
};
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBibleInstallPhase, LocalBibleInstallProgress,
    LocalBiblePackStatus,
};
use kairo_domain::scripture_trace::{ScriptureTraceRecord, TraceStatus};
use kairo_domain::status::AutoPresentDelaySec;
use kairo_ui_kit::components::PopoverTriggerExt;
use kairo_ui_kit::components::{
    SegmentOption, SegmentedControl, confirm, dialogs::alert, messages,
};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use std::collections::{HashMap, HashSet};

/// How many traces the timing panel reads.
pub const TRACE_LIMIT: usize = 100;

/// Element ids, for tests.
pub mod ids {
    /// The default translation picker.
    pub const TRANSLATION: &str = "settings-translation";
    /// Verse numbers on slides.
    pub const VERSE_NUMBERS: &str = "settings-scripture-numbers";
    /// Automation.
    pub const AUTO: &str = "settings-scripture-auto";
    /// Confidence.
    pub const CONFIDENCE: &str = "settings-scripture-confidence";
    /// Safety delay.
    pub const DELAY: &str = "settings-scripture-delay";
    /// Debounce.
    pub const DEBOUNCE: &str = "settings-scripture-debounce";
    /// Context window.
    pub const CONTEXT: &str = "settings-scripture-context";
    /// Library expand/collapse.
    pub const LIBRARY_TOGGLE: &str = "settings-library-toggle";
    /// Download all.
    pub const DOWNLOAD_ALL: &str = "settings-library-download-all";
    /// The bulk error.
    pub const BULK_ERROR: &str = "settings-library-error";
    /// A pack row: `settings-pack-<ID>`.
    pub fn pack(id: &str) -> String {
        format!("settings-pack-{id}")
    }
    /// A pack's action: `settings-pack-action-<ID>`.
    pub fn pack_action(id: &str) -> String {
        format!("settings-pack-action-{id}")
    }
    /// A pack's detail line: `settings-pack-detail-<ID>`.
    pub fn pack_detail(id: &str) -> String {
        format!("settings-pack-detail-{id}")
    }
    /// The timing filter.
    pub const TRACE_FILTER: &str = "settings-trace-filter";
    /// Refresh timings.
    pub const TRACE_REFRESH: &str = "settings-trace-refresh";
    /// The timing list's empty line.
    pub const TRACE_EMPTY: &str = "settings-trace-empty";
    /// A trace row: `settings-trace-<correlation id>`.
    pub fn trace(id: &str) -> String {
        format!("settings-trace-{id}")
    }
}

/// The Bible library's state.
#[derive(Clone, Debug)]
pub struct LibraryPane {
    /// `listInstalledLocalBiblePacks`.
    pub installed: Vec<InstalledLocalBiblePack>,
    /// Each pack row's state, by upper-case id.
    pub rows: HashMap<String, PackState>,
    /// The English list is open.
    pub expanded: bool,
    /// Download all is running.
    pub bulk_busy: bool,
    /// Download all's error.
    pub bulk_error: Option<String>,
}

impl Default for LibraryPane {
    fn default() -> Self {
        Self {
            installed: Vec::new(),
            rows: HashMap::new(),
            expanded: true,
            bulk_busy: false,
            bulk_error: None,
        }
    }
}

/// The verse-timing panel's state.
#[derive(Clone, Debug)]
pub struct TracePane {
    /// The last read.
    pub records: Vec<ScriptureTraceRecord>,
    /// The filter.
    pub filter: TraceFilter,
    /// The first read has not answered.
    pub loading: bool,
    /// Rows opened to show their steps.
    pub open: HashSet<String>,
}

impl Default for TracePane {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            filter: TraceFilter::All,
            loading: true,
            open: HashSet::new(),
        }
    }
}

fn key(id: &str) -> String {
    id.to_uppercase()
}

impl SettingsWindow {
    // ── Default translation ─────────────────────────────────────────────

    /// Picks the default Bible, downloading it first when it needs one.
    pub fn pick_translation(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if !needs_download(&self.translations, &id) {
            self.settings.scripture.default_translation = id;
            cx.notify();
            return;
        }
        let Some(option) = self.translations.iter().find(|t| t.id == id).cloned() else {
            return;
        };
        let request = messages::settings_download_translation(
            &option.id,
            &option.name,
            option.download_approx.as_deref(),
        );
        let entity = cx.entity();
        confirm(request, window, cx, move |yes, window, cx| {
            if yes {
                entity.update(cx, |this, cx| this.download_and_pick(id, window, cx));
            }
        });
    }

    fn download_and_pick(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.downloading_translation = true;
        let download = self.backend.download_pack(id.clone());
        let refresh = self.backend.translations();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let result = download.await;
            let refreshed = match &result {
                Ok(_) => refresh.await.ok(),
                Err(_) => None,
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.downloading_translation = false;
                match result {
                    Ok(_) => {
                        if let Some(list) = refreshed {
                            this.translations = list;
                        }
                        this.settings.scripture.default_translation = id;
                        this.open_library(cx);
                    }
                    Err(error) => alert(error, window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ── Library ─────────────────────────────────────────────────────────

    /// Reads the installed packs and each row's status.
    pub(crate) fn open_library(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.installed_packs();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                // A failed list still leaves the downloadable rows.
                if let Ok(installed) = result {
                    this.library.installed = installed;
                }
                this.load_pack_statuses(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn load_pack_statuses(&mut self, cx: &mut Context<Self>) {
        let lib = library(&self.translations, &self.library.installed);
        for row in lib.packs {
            let id = key(&row.id);
            let state = self.library.rows.entry(id.clone()).or_default();
            if state.status.is_none() {
                state.loading = true;
            }
            let request = self.backend.pack_status(row.id.clone());
            cx.spawn(async move |this, cx| {
                let result = request.await;
                let _ = this.update(cx, |this, cx| {
                    let state = this.library.rows.entry(id).or_default();
                    match result {
                        Ok(status) => state.accept(status),
                        Err(error) => state.error = Some(error),
                    }
                    state.loading = false;
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub(crate) fn on_pack_progress(
        &mut self,
        progress: LocalBibleInstallProgress,
        cx: &mut Context<Self>,
    ) {
        let id = key(&progress.translation);
        let ready = progress.phase == LocalBibleInstallPhase::Ready;
        self.library
            .rows
            .entry(id.clone())
            .or_default()
            .progress(progress);
        if ready {
            for option in &mut self.translations {
                if key(&option.id) == id {
                    option.available = true;
                    option.requires_api_key = false;
                }
            }
            self.refresh_translations(cx);
            self.open_library(cx);
        }
    }

    /// Downloads one pack.
    pub fn download_pack(&mut self, id: String, cx: &mut Context<Self>) {
        let row = self.library.rows.entry(key(&id)).or_default();
        if row.busy.is_some() {
            return;
        }
        row.busy = Some(PackBusy::Download);
        row.error = None;
        let request = self.backend.download_pack(id.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                let row = this.library.rows.entry(key(&id)).or_default();
                row.busy = None;
                match result {
                    Ok(next) => {
                        let installation = row.status.as_ref().and_then(|s| s.installation.clone());
                        row.status = Some(LocalBiblePackStatus {
                            installation,
                            ..next
                        });
                        this.refresh_translations(cx);
                        this.open_library(cx);
                    }
                    Err(error) => {
                        row.error = Some(error);
                        this.reload_pack(id, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn reload_pack(&mut self, id: String, cx: &mut Context<Self>) {
        let request = self.backend.pack_status(id.clone());
        cx.spawn(async move |this, cx| {
            if let Ok(status) = request.await {
                let _ = this.update(cx, |this, cx| {
                    this.library
                        .rows
                        .entry(key(&id))
                        .or_default()
                        .accept(status);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Asks, then removes one pack.
    pub fn ask_remove_pack(
        &mut self,
        id: String,
        name: String,
        downloadable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.library.rows.entry(key(&id)).or_default().error = None;
        let detail = if downloadable {
            format!(
                "{name} will be removed from this computer. You can download it again whenever you need it."
            )
        } else {
            format!(
                "{name} will be removed from this computer. This Bible is no longer available for download in Kairo."
            )
        };
        let request = kairo_ui_kit::components::ConfirmRequest::new(
            format!("Remove {id} download?"),
            "Remove download",
        )
        .detail(detail)
        .destructive();
        let entity = cx.entity();
        confirm(request, window, cx, move |yes, _, cx| {
            if yes {
                entity.update(cx, |this, cx| this.remove_pack(id, cx));
            }
        });
    }

    /// Removes one pack.
    pub fn remove_pack(&mut self, id: String, cx: &mut Context<Self>) {
        let row = self.library.rows.entry(key(&id)).or_default();
        if row.busy.is_some() {
            return;
        }
        row.busy = Some(PackBusy::Remove);
        row.error = None;
        let request = self.backend.remove_pack(id.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                let row = this.library.rows.entry(key(&id)).or_default();
                row.busy = None;
                match result {
                    Ok(status) => {
                        row.status = Some(status);
                        this.refresh_translations(cx);
                        this.open_library(cx);
                    }
                    Err(error) => {
                        row.error = Some(error);
                        this.reload_pack(id, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Download all: each pending pack in turn, stopping at the first error.
    pub fn download_all(&mut self, cx: &mut Context<Self>) {
        if self.library.bulk_busy {
            return;
        }
        let pending =
            library(&self.translations, &self.library.installed).pending(&self.library.installed);
        self.library.bulk_busy = true;
        self.library.bulk_error = None;
        let backend = self.backend.clone();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut error = None;
            for id in pending {
                match backend.pack_status(id.clone()).await {
                    Ok(status) if status.installed => continue,
                    Ok(_) => {
                        if let Err(e) = backend.download_pack(id).await {
                            error = Some(e);
                            break;
                        }
                    }
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
            }
            let refreshed = backend.translations().await;
            let _ = this.update(cx, |this, cx| {
                match refreshed {
                    Ok(list) => this.translations = list,
                    Err(e) => error = error.take().or(Some(e)),
                }
                this.library.bulk_error = error;
                this.library.bulk_busy = false;
                this.open_library(cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ── Verse timing ────────────────────────────────────────────────────

    pub(crate) fn load_traces(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.recent_traces(TRACE_LIMIT);
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                // A failed read shows the empty state, as the original does.
                this.traces.records = result.unwrap_or_default();
                this.traces.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    // ── Drawing ─────────────────────────────────────────────────────────

    pub(crate) fn render_scripture(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let sc = self.settings.scripture.clone();
        let entity = cx.entity();
        let (missing, ready) = translation_choices(&self.translations, &sc.default_translation);
        let picker = {
            let entity = entity.clone();
            let saved = sc.default_translation.clone();
            Button::new(ids::TRANSLATION)
                .small()
                .outline()
                .w(px(168.))
                .label(sc.default_translation.clone())
                .dropdown_caret(true)
                .disabled(self.downloading_translation)
                .accessibility_label("Default bible translation")
                .popover_trigger()
                .dropdown_menu(move |mut menu, _, _| {
                    if let Some(missing) = &missing {
                        menu = menu.item(
                            PopupMenuItem::new(format!(
                                "{} — {} (not installed)",
                                missing.id, missing.name
                            ))
                            .disabled(true)
                            .checked(true),
                        );
                    }
                    for option in &ready {
                        let entity = entity.clone();
                        let id = option.id.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!("{} — {}", option.id, option.name))
                                .checked(option.id == saved)
                                .on_click(move |_, window, cx| {
                                    let id = id.clone();
                                    entity.update(cx, |this, cx| {
                                        this.pick_translation(id, window, cx)
                                    })
                                }),
                        );
                    }
                    menu
                })
        };
        let default_group = widgets::group(
            Some("Default Bible"),
            None,
            vec![
                widgets::row(
                    "Translation",
                    Some(if self.translations_loading {
                        "Checking available Bibles…"
                    } else {
                        "Used for new searches and auto-detection. Add more in Bible library below."
                    }),
                    picker,
                    cx,
                ),
                widgets::row(
                    "Verse numbers",
                    Some("Include numbers on slides"),
                    widgets::toggle(
                        ids::VERSE_NUMBERS,
                        "Verse numbers",
                        sc.show_verse_numbers,
                        {
                            let entity = entity.clone();
                            move |on, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.settings.scripture.show_verse_numbers = on;
                                    cx.notify();
                                })
                            }
                        },
                    ),
                    cx,
                ),
            ],
            cx,
        );

        let library = self.render_library(window, cx);
        let library_group = widgets::group(
            Some("Bible library"),
            Some(
                "Download Bibles for offline use. Translations marked Online are available through your internet connection.",
            ),
            vec![library],
            cx,
        );

        let number = |id: &'static str,
                      field: &Entity<gpui_kit::component::input::InputState>,
                      label: &'static str| {
            h_flex()
                .gap_1p5()
                .items_center()
                .child(
                    div()
                        .w_16()
                        .h_7()
                        .rounded(radius::base())
                        .bg(k.surface_tertiary)
                        .px_2()
                        .font_family(text::MONO_FAMILY)
                        .child(
                            Input::new(field)
                                .id(id)
                                .appearance(false)
                                .small()
                                .aria_label(label),
                        ),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("sec"),
                )
        };
        let delay = sc.auto_present_delay_sec;
        let auto_rows = v_flex()
            .when(!sc.auto_mode, |this| this.opacity(0.4))
            .child(widgets::stacked(
                "Confidence",
                Some("Higher is stricter. 0.70–0.85 suits most sermons."),
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div().flex_1().child(
                            Slider::new(&self.confidence)
                                .disabled(!sc.auto_mode)
                                .accessibility_label("Confidence threshold range slider"),
                        ),
                    )
                    .child(
                        div()
                            .id(ids::CONFIDENCE)
                            .w_10()
                            .text_right()
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(format!("{:.2}", sc.confidence_threshold)),
                    ),
                cx,
            ))
            .child(widgets::row(
                "Safety delay",
                Some("Time to cancel before a verse goes live. The largest part of the wait."),
                SegmentedControl::new(ids::DELAY, "Safety delay")
                    .fit()
                    .compact(true)
                    .options([
                        SegmentOption::new("0", "Instant"),
                        SegmentOption::new("1", "1s"),
                        SegmentOption::new("2", "2s"),
                        SegmentOption::new("3", "3s"),
                    ])
                    .selected(SharedString::from((delay as u8).to_string()))
                    .on_change({
                        let entity = entity.clone();
                        let enabled = sc.auto_mode;
                        move |key, _, cx| {
                            if !enabled {
                                return;
                            }
                            let value = match key.as_ref() {
                                "0" => AutoPresentDelaySec::Zero,
                                "2" => AutoPresentDelaySec::Two,
                                "3" => AutoPresentDelaySec::Three,
                                _ => AutoPresentDelaySec::One,
                            };
                            entity.update(cx, |this, cx| {
                                this.settings.scripture.auto_present_delay_sec = value;
                                cx.notify();
                            })
                        }
                    }),
                cx,
            ))
            .child(widgets::row(
                "Debounce",
                Some("Minimum gap between suggestions"),
                number(
                    ids::DEBOUNCE,
                    &self.debounce,
                    "Detection debounce duration in seconds",
                ),
                cx,
            ))
            .child(widgets::row(
                "Context window",
                Some("Transcript sent for analysis"),
                number(
                    ids::CONTEXT,
                    &self.context_window,
                    "Context window size in seconds",
                ),
                cx,
            ));
        let auto_group = widgets::group(
            Some("Auto-detection"),
            None,
            vec![
                widgets::row(
                    "Automation",
                    Some(
                        "Show confident verses on screen by themselves. Off, they wait as suggestions.",
                    ),
                    widgets::toggle(ids::AUTO, "Automation", sc.auto_mode, {
                        let entity = entity.clone();
                        move |on, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.settings.scripture.auto_mode = on;
                                cx.notify();
                            })
                        }
                    }),
                    cx,
                ),
                auto_rows.into_any_element(),
            ],
            cx,
        );

        let timing = self.render_traces(cx);
        let timing_group = widgets::group(
            Some("Verse timing"),
            Some(
                "How long each detected verse took to appear in Kairo this session, excluding the safety delay. Open a verse to see which step was slow.",
            ),
            vec![timing],
            cx,
        );

        v_flex()
            .gap_5()
            .child(default_group)
            .child(library_group)
            .child(auto_group)
            .child(timing_group)
            .children(
                self.save_error
                    .clone()
                    .map(|e| widgets::notice("settings-save-error", e, cx)),
            )
            .child(widgets::save_bar(
                Section::Scripture,
                self.saved == Some(Section::Scripture),
                cx.listener(|this, _, _, cx| this.save(Section::Scripture, cx)),
                cx,
            ))
            .into_any_element()
    }

    fn render_library(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let lib = library(&self.translations, &self.library.installed);
        let pending = lib.pending(&self.library.installed);
        let expanded = self.library.expanded;
        let toggle = cx.listener(|this, _, _, cx| {
            this.library.expanded = !this.library.expanded;
            cx.notify();
        });
        let header = h_flex()
            .gap_3()
            .items_center()
            .px_4()
            .py_3()
            .child(
                div()
                    .flex_1()
                    .text_size(text::SM.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("English"),
            )
            .when(!pending.is_empty(), |this| {
                this.child(widgets::button(
                    ids::DOWNLOAD_ALL,
                    if self.library.bulk_busy {
                        "Downloading…"
                    } else {
                        "Download all"
                    },
                    self.library.bulk_busy,
                    cx.listener(|this, _, _, cx| this.download_all(cx)),
                ))
            })
            .child(
                div()
                    .id(ids::LIBRARY_TOGGLE)
                    .test_support()
                    .role(Role::Button)
                    .aria_label(if expanded {
                        "Collapse English Bibles"
                    } else {
                        "Expand English Bibles"
                    })
                    .aria_expanded(expanded)
                    .tab_index(0)
                    .size_7()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::md())
                    .text_color(k.text(TextStep::T400))
                    .cursor_pointer()
                    .hover(|s| s.bg(k.surface_tertiary))
                    .child(Icon::new(if expanded {
                        KairoIcon::ChevronUp
                    } else {
                        KairoIcon::ChevronDown
                    }))
                    .on_click(toggle),
            );
        let mut rows: Vec<AnyElement> = Vec::new();
        if expanded {
            for (id, name) in &lib.bundled {
                rows.push(
                    self.pack_shell(
                        id,
                        name,
                        None,
                        false,
                        Some(
                            div()
                                .size_7()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(k.text_primary)
                                .child(Icon::new(KairoIcon::CheckCircle).size(px(18.)))
                                .into_any_element(),
                        ),
                        None,
                        cx,
                    ),
                );
            }
            for pack in &lib.packs {
                if let Some(row) = self.render_pack(pack, cx) {
                    rows.push(row);
                }
            }
            for (id, name) in &lib.online {
                rows.push(self.pack_shell(
                    id,
                    name,
                    None,
                    false,
                    Some(widgets::muted("Online", cx)),
                    None,
                    cx,
                ));
            }
        }
        v_flex()
            .py_2()
            .child(header)
            .when_some(self.library.bulk_error.clone(), |this, error| {
                this.child(
                    div()
                        .id(ids::BULK_ERROR)
                        .test_support()
                        .role(Role::Alert)
                        .px_4()
                        .pb_2()
                        .text_size(text::XS.size)
                        .text_color(k.hue(kairo_ui_kit::Hue::Amber))
                        .child(error),
                )
            })
            .children(rows)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn pack_shell(
        &self,
        id: &str,
        name: &str,
        detail: Option<(String, bool)>,
        live: bool,
        action: Option<AnyElement>,
        footer: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        v_flex()
            .id(SharedString::from(ids::pack(id)))
            .test_support()
            .px_4()
            .py_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                h_flex()
                                    .text_size(text::SM.size)
                                    .text_color(k.text_primary)
                                    .child(
                                        div()
                                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                                            .child(id.to_owned()),
                                    )
                                    .child(format!(" · {name}")),
                            )
                            .when_some(detail, |this, (detail, warn)| {
                                this.child(
                                    h_flex()
                                        .id(SharedString::from(ids::pack_detail(id)))
                                        .test_support()
                                        .role(if warn {
                                            Role::Alert
                                        } else if live {
                                            Role::Status
                                        } else {
                                            Role::Paragraph
                                        })
                                        .aria_label(detail.clone())
                                        .mt_0p5()
                                        .gap_1()
                                        .text_size(text::XS.size)
                                        .text_color(if warn {
                                            k.hue(kairo_ui_kit::Hue::Amber)
                                        } else {
                                            k.text(TextStep::T400)
                                        })
                                        .when(warn, |this| {
                                            this.child(Icon::new(KairoIcon::AlertTriangle).size_3())
                                        })
                                        .child(detail),
                                )
                            }),
                    )
                    .children(action),
            )
            .children(footer)
            .into_any_element()
    }

    fn render_pack(
        &self,
        pack: &crate::model::PackRow,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = *cx.kairo();
        let state = self
            .library
            .rows
            .get(&key(&pack.id))
            .cloned()
            .unwrap_or(PackState {
                loading: true,
                ..Default::default()
            });
        if state.hidden(pack.downloadable) {
            return None;
        }
        let name = if pack.name.is_empty() {
            pack.id.clone()
        } else {
            pack.name.clone()
        };
        let installing = state.installing();
        let failure = state.failure();
        let detail_text = state.detail(pack.downloadable, pack.size.as_deref());
        let detail = if failure.is_some() {
            Some((detail_text, true))
        } else if installing || state.loading {
            Some((detail_text, false))
        } else {
            None
        };
        let entity = cx.entity();
        let action: Option<AnyElement> = if installing {
            Some(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(Icon::new(KairoIcon::Loader).size_3())
                    .child("Installing…")
                    .into_any_element(),
            )
        } else if state.installed() {
            let id = pack.id.clone();
            let remove_name = name.clone();
            let downloadable = pack.downloadable;
            Some(
                Button::new(SharedString::from(ids::pack_action(&pack.id)))
                    .ghost()
                    .small()
                    .icon(Icon::new(KairoIcon::CheckCircle))
                    .disabled(state.busy.is_some() || state.loading)
                    .accessibility_label(format!("Actions for {name}"))
                    .tooltip("Installed · manage download")
                    .popover_trigger()
                    .dropdown_menu(move |menu, _, _| {
                        let entity = entity.clone();
                        let id = id.clone();
                        let remove_name = remove_name.clone();
                        menu.item(PopupMenuItem::new("Remove download…").on_click(
                            move |_, window, cx| {
                                let (id, name) = (id.clone(), remove_name.clone());
                                entity.update(cx, |this, cx| {
                                    this.ask_remove_pack(id, name, downloadable, window, cx)
                                })
                            },
                        ))
                    })
                    .into_any_element(),
            )
        } else if pack.downloadable {
            let id = pack.id.clone();
            let verb = if failure.is_some() {
                "Retry download"
            } else {
                "Download"
            };
            Some(
                Button::new(SharedString::from(ids::pack_action(&pack.id)))
                    .ghost()
                    .small()
                    .icon(Icon::new(KairoIcon::Download))
                    .disabled(state.busy.is_some() || state.loading)
                    .accessibility_label(format!("{verb} {name}"))
                    .tooltip(format!(
                        "{verb} {}{}",
                        pack.id,
                        pack.size
                            .as_ref()
                            .map(|s| format!(" ({s})"))
                            .unwrap_or_default()
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| this.download_pack(id.clone(), cx)))
                    .into_any_element(),
            )
        } else {
            None
        };
        let footer = installing.then(|| {
            let percent = state.percent();
            let downloading = state
                .status
                .as_ref()
                .and_then(|s| s.installation.as_ref())
                .is_some_and(|i| i.phase == LocalBibleInstallPhase::Downloading);
            v_flex()
                .mt_2()
                .child(
                    div()
                        .id(SharedString::from(format!(
                            "settings-pack-progress-{}",
                            pack.id
                        )))
                        .test_support()
                        .role(Role::ProgressIndicator)
                        .aria_label(format!(
                            "{} {}",
                            if downloading {
                                "Downloading"
                            } else {
                                "Installing"
                            },
                            pack.id
                        ))
                        .h_1()
                        .rounded_full()
                        .overflow_hidden()
                        .bg(k.surface_border)
                        .when_some(percent.filter(|_| downloading), |this, percent| {
                            this.child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .bg(k.accent(Step::S400))
                                    .w(relative(percent as f32 / 100.0)),
                            )
                        }),
                )
                .child(
                    div()
                        .mt_1p5()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child("You can keep using Kairo while this finishes."),
                )
                .into_any_element()
        });
        Some(self.pack_shell(&pack.id, &name, detail, true, action, footer, cx))
    }

    fn render_traces(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let records = self.traces.records.clone();
        let (typical, slow, presented) = trace_summary(&records);
        let stat = |label: &'static str, value: String, hint: String| {
            v_flex()
                .min_w_0()
                .flex_1()
                .px(px(14.))
                .py(px(10.))
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child(label),
                )
                .child(
                    div()
                        .mt_0p5()
                        .font_family(text::MONO_FAMILY)
                        .text_size(px(18.))
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(k.text_primary)
                        .child(value),
                )
                .child(
                    div()
                        .mt_0p5()
                        .truncate()
                        .text_size(text::MICRO.size)
                        .text_color(k.text(TextStep::T600))
                        .child(hint),
                )
        };
        let filter = self.traces.filter;
        let visible: Vec<_> = records
            .iter()
            .filter(|r| filter.keeps(r))
            .cloned()
            .collect();
        let body: AnyElement = if self.traces.loading {
            div()
                .px(px(14.))
                .pb_4()
                .pt_2()
                .child(widgets::muted("Loading…", cx))
                .into_any_element()
        } else if visible.is_empty() {
            div()
                .id(ids::TRACE_EMPTY)
                .test_support()
                .px(px(14.))
                .pb_4()
                .pt_2()
                .child(widgets::muted(
                    if records.is_empty() {
                        "No verses detected yet this session. Timings appear here once auto-detection finds one."
                    } else {
                        "Nothing matches this filter."
                    },
                    cx,
                ))
                .into_any_element()
        } else {
            v_flex()
                .id("settings-trace-list")
                .max_h(px(320.))
                .overflow_y_scroll()
                .children(visible.iter().map(|r| self.render_trace(r, cx)))
                .into_any_element()
        };
        v_flex()
            .child(
                h_flex()
                    .child(stat(
                        "Typical",
                        typical,
                        "Half of verses were faster".into(),
                    ))
                    .child(stat("Slow", slow, "Only 1 in 20 was slower".into()))
                    .child(stat(
                        "Presented",
                        presented.to_string(),
                        format!("of {} detected", records.len()),
                    )),
            )
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .px(px(14.))
                    .py_2()
                    .child(
                        SegmentedControl::new(ids::TRACE_FILTER, "Filter verses")
                            .fit()
                            .compact(true)
                            .options(
                                [
                                    TraceFilter::All,
                                    TraceFilter::Presented,
                                    TraceFilter::Issues,
                                ]
                                .map(|f| SegmentOption::new(f.label(), f.label())),
                            )
                            .selected(filter.label())
                            .on_change({
                                let entity = cx.entity();
                                move |key, _, cx| {
                                    let next = match key.as_ref() {
                                        "Presented" => TraceFilter::Presented,
                                        "Not shown" => TraceFilter::Issues,
                                        _ => TraceFilter::All,
                                    };
                                    entity.update(cx, |this, cx| {
                                        this.traces.filter = next;
                                        cx.notify();
                                    })
                                }
                            }),
                    )
                    .child(
                        Button::new(ids::TRACE_REFRESH)
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::RefreshCw))
                            .accessibility_label("Refresh timings")
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| this.load_traces(cx))),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    fn render_trace(&self, record: &ScriptureTraceRecord, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let trace = &record.trace;
        let id = trace.correlation_id.clone();
        let open = self.traces.open.contains(&id);
        let status = trace.status;
        let tone = match status {
            TraceStatus::Active => k.notice_text,
            TraceStatus::Presented => k.success_text,
            TraceStatus::Failed => k.danger_text,
            TraceStatus::Cancelled | TraceStatus::Superseded => k.text(TextStep::T500),
        };
        let steps = stages(&record.metrics);
        let longest = steps.iter().map(|s| s.1).fold(1.0f64, f64::max);
        let slowest = record
            .metrics
            .slowest_stage
            .as_ref()
            .map(|s| s.name.clone());
        let toggle_id = id.clone();
        v_flex()
            .child(
                h_flex()
                    .id(SharedString::from(ids::trace(&id)))
                    .test_support()
                    .role(Role::Button)
                    .aria_expanded(open)
                    .aria_label(
                        trace
                            .reference
                            .clone()
                            .unwrap_or_else(|| trace.source_text.clone()),
                    )
                    .tab_index(0)
                    .gap(px(10.))
                    .items_center()
                    .px(px(14.))
                    .py_2()
                    .cursor_pointer()
                    .hover(|s| s.bg(k.surface_secondary))
                    .child(
                        Icon::new(if open {
                            KairoIcon::ChevronDown
                        } else {
                            KairoIcon::ChevronRight
                        })
                        .size(px(11.))
                        .text_color(k.text(TextStep::T600)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .text_color(k.text_primary)
                                    .child(
                                        trace
                                            .reference
                                            .clone()
                                            .unwrap_or_else(|| trace.source_text.clone()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .mt_0p5()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child(format!("{} · ", resolver_label(trace.resolver)))
                                    .child(div().text_color(tone).child(status_label(status))),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T300))
                            .child(ms(headline(record))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.traces.open.remove(&toggle_id) {
                            this.traces.open.insert(toggle_id.clone());
                        }
                        cx.notify();
                    })),
            )
            .when(open, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .pl(px(34.))
                        .pr(px(14.))
                        .pb_3()
                        .when(steps.is_empty(), |this| {
                            this.child(widgets::muted("No timing recorded for this verse.", cx))
                        })
                        .children(steps.iter().map(|(label, value, deliberate)| {
                            let slow = slowest.as_deref() == Some(*label);
                            let color = if slow {
                                k.hue(kairo_ui_kit::Hue::Amber)
                            } else {
                                k.text(TextStep::T400)
                            };
                            let bar = if *deliberate {
                                k.surface_border
                            } else if slow {
                                k.hue(kairo_ui_kit::Hue::Amber)
                            } else {
                                k.accent(Step::S500)
                            };
                            h_flex()
                                .gap(px(10.))
                                .items_center()
                                .child(
                                    div()
                                        .w(px(124.))
                                        .truncate()
                                        .text_size(text::LABEL.size)
                                        .text_color(color)
                                        .child(*label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .h_1p5()
                                        .rounded_full()
                                        .overflow_hidden()
                                        .bg(k.surface_tertiary)
                                        .child(
                                            div()
                                                .h_full()
                                                .rounded_full()
                                                .bg(bar)
                                                .w(relative((value / longest).max(0.02) as f32)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w(px(52.))
                                        .text_right()
                                        .font_family(text::MONO_FAMILY)
                                        .text_size(text::LABEL.size)
                                        .text_color(if slow {
                                            color
                                        } else {
                                            k.text(TextStep::T300)
                                        })
                                        .child(ms(Some(*value))),
                                )
                        }))
                        .when_some(trace.failure_reason, |this, reason| {
                            this.child(
                                div()
                                    .pt_1()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.danger_text)
                                    .child(reason.as_str().to_owned()),
                            )
                        }),
                )
            })
            .into_any_element()
    }
}
