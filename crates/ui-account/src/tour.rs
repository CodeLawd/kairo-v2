//! The welcome tour (`TourSlides.tsx`): one large headline per slide,
//! three short bullets, and a mock-up with accent callouts. Each mock-up
//! plays one small story (select then present, type then result, countdown).
//! Every frame is a function of [`Clock`], so the wizard drives it with
//! animation frames and reduced motion shows the finished slide.

use crate::ob::{self, Clock};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, FontWeight, HighlightStyle, Hsla, IntoElement, ParentElement, SharedString,
    Styled, StyledText, div, prelude::FluentBuilder, px, relative,
};
use kairo_ui_kit::KairoIcon;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::onboarding::{OnboardingColors, mix};
use kairo_ui_kit::tokens::{TextStep, text};

/// One slide.
#[derive(Clone, Copy, Debug)]
pub struct TourSlide {
    /// Stable id.
    pub id: &'static str,
    /// The headline.
    pub headline: &'static str,
    /// The line under it.
    pub subhead: &'static str,
    /// Three bullets.
    pub bullets: [&'static str; 3],
}

/// `TOUR_SLIDES`.
pub const TOUR_SLIDES: [TourSlide; 6] = [
    TourSlide {
        id: "present",
        headline: "Present",
        subhead: "your whole service from one place",
        bullets: [
            "Build a playlist for the run of service",
            "Send scripture, songs and media to the screen",
            "Clear text or background in one click",
        ],
    },
    TourSlide {
        id: "scripture",
        headline: "Scripture",
        subhead: "any verse, in seconds",
        bullets: [
            "Type a reference or search by words",
            "Catch references as they are spoken",
            "Offline translations work with no setup",
        ],
    },
    TourSlide {
        id: "songs",
        headline: "Songs",
        subhead: "lyrics ready for every service",
        bullets: [
            "Search and import lyrics",
            "Arrange verses, choruses and bridges",
            "Your library lives in a folder you own",
        ],
    },
    TourSlide {
        id: "documents",
        headline: "Slides",
        subhead: "sermon slides and handouts, ready to show",
        bullets: [
            "Import PowerPoint and PDF files",
            "Videos inside your slides play too",
            "Step through slides live, one click each",
        ],
    },
    TourSlide {
        id: "style",
        headline: "Themes & Media",
        subhead: "make every slide look like your church",
        bullets: [
            "Design themes for scripture and lyrics",
            "Keep backgrounds and videos in one dock",
            "Change the look without touching content",
        ],
    },
    TourSlide {
        id: "screens",
        headline: "Screens",
        subhead: "audience and stage, set up your way",
        bullets: [
            "Audience screens for the room",
            "Stage displays with next slide, clock and timer",
            "Style each screen with its own theme",
        ],
    },
];

/// Text with `**bold**` runs, as the callouts' `<b>`.
pub fn rich(source: &str, bold: Hsla) -> StyledText {
    let mut plain = String::new();
    let mut ranges = Vec::new();
    for (i, part) in source.split("**").enumerate() {
        let start = plain.len();
        plain.push_str(part);
        if i % 2 == 1 {
            ranges.push((
                start..plain.len(),
                HighlightStyle {
                    color: Some(bold),
                    font_weight: Some(FontWeight::SEMIBOLD),
                    ..Default::default()
                },
            ));
        }
    }
    StyledText::new(plain).with_highlights(ranges)
}

/// What a slide's art needs.
pub struct Art<'a> {
    /// The slide's clock.
    pub clock: Clock,
    /// Setup colours.
    pub ob: &'a OnboardingColors,
    /// App tokens.
    pub cx: &'a App,
}

impl Art<'_> {
    fn k(&self) -> &kairo_ui_kit::KairoColors {
        self.cx.kairo()
    }

    /// `Window`: a borderless mock window.
    fn window(&self, title: Option<&'static str>, base: f32, body: impl IntoElement) -> AnyElement {
        let k = self.k();
        let bar = h_flex()
            .items_center()
            .gap(px(6.))
            .pt(px(9.))
            .px(px(12.))
            .pb(px(5.))
            .children((0..3).map(|_| div().size(px(8.)).rounded_full().bg(self.ob.card[2])))
            .when_some(title, |el, title| {
                el.child(
                    div()
                        .ml_2()
                        .text_size(px(10.))
                        .text_color(k.text(TextStep::T500))
                        .child(title),
                )
            });
        self.clock
            .ob_in(
                v_flex()
                    .overflow_hidden()
                    .rounded(px(16.))
                    .bg(self.ob.card[0])
                    .text_size(px(11.))
                    .text_color(k.text(TextStep::T300))
                    .child(bar)
                    .child(body),
                0,
                base,
                64.0,
            )
            .into_any_element()
    }

    /// `Callout`: an accent line beside one text run.
    fn callout(&self, base: f32, source: &str) -> AnyElement {
        let k = self.k();
        let line = self.clock.line(base + 0.0);
        self.clock
            .ob_in(
                h_flex()
                    .items_stretch()
                    .gap_3()
                    .text_size(px(14.))
                    .line_height(relative(1.3))
                    .text_color(k.text(TextStep::T300))
                    .child(
                        div()
                            .w(px(2.))
                            .flex_shrink_0()
                            .child(div().w_full().h(relative(line)).bg(self.ob.blue)),
                    )
                    .child(div().child(rich(source, k.text_primary))),
                0,
                base,
                20.0,
            )
            .into_any_element()
    }

    fn side(&self) -> gpui_kit::Div {
        v_flex()
            .pt(px(6.))
            .px(px(8.))
            .pb(px(10.))
            .bg(self.ob.card[1])
            .rounded_tr(px(10.))
    }

    /// `.onboarding-audience-slide`: a 16:9 frame.
    fn slide(&self, bg: Hsla, width: f32) -> gpui_kit::Div {
        v_flex()
            .w(px(width))
            .h(px(width * 9.0 / 16.0))
            .items_center()
            .justify_center()
            .rounded(px(4.))
            .bg(bg)
            .text_color(self.ob.slide_text.0)
    }

    fn row(
        &self,
        label: &'static str,
        i: usize,
        base: f32,
        selected: Option<(f32, Hsla, Hsla)>,
    ) -> AnyElement {
        let mut row = div()
            .rounded(px(4.))
            .px(px(6.))
            .py(px(6.))
            .truncate()
            .child(label);
        if let Some((select_base, fill, text_color)) = selected {
            let t = self.clock.select(select_base);
            row = row.bg(mix(fill.opacity(0.0), fill, t)).text_color(mix(
                self.k().text(TextStep::T300),
                text_color,
                t,
            ));
        }
        self.clock
            .rise(div().child(row), i, base)
            .into_any_element()
    }

    /// Rows arrive, the scripture item is selected, then it lands on the
    /// audience slide.
    pub fn present(&self) -> AnyElement {
        let k = self.k();
        let items = [
            "Welcome",
            "Way Maker",
            "Goodness of God",
            "John 3:16–17",
            "Sermon",
            "Closing",
        ];
        let teal = k.hue(kairo_ui_kit::Hue::Cyan);
        let side = self
            .side()
            .w(relative(0.4))
            .child(
                div()
                    .px(px(6.))
                    .pb(px(6.))
                    .text_size(px(9.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text(TextStep::T500))
                    .child("SUNDAY MORNING"),
            )
            .children(items.iter().enumerate().map(|(i, name)| {
                self.row(
                    name,
                    i,
                    300.0,
                    (i == 3).then_some((900.0, teal, k.on_accent)),
                )
            }));
        let preview = v_flex()
            .flex_1()
            .pb_3()
            .pr_3()
            .child(
                self.clock.pop(
                    self.slide(self.ob.slide_bg, 300.0)
                        .border_2()
                        .border_color(k.live)
                        .px_4()
                        .child(div().text_size(px(13.)).text_center().child(
                            "For God so loved the world, that he gave his only begotten Son",
                        ))
                        .child(
                            div()
                                .mt_2()
                                .text_size(px(8.))
                                .text_color(k.text(TextStep::T400))
                                .child("JOHN 3:16"),
                        ),
                    0,
                    1050.0,
                ),
            )
            .child(
                h_flex()
                    .mt_2()
                    .gap(px(6.))
                    .children([0usize, 1, 2].map(|i| {
                        self.clock.pop(
                            div()
                                .flex_1()
                                .h(px(54.))
                                .rounded(px(2.))
                                .bg(self.ob.art[[0, 1, 2][i]]),
                            i,
                            450.0,
                        )
                    })),
            );
        v_flex()
            .gap(px(18.))
            .child(self.callout(260.0, "Build a **playlist** to set your run of service"))
            .child(self.window(
                Some("Operator"),
                120.0,
                h_flex().gap_3().child(side).child(preview),
            ))
            .child(self.callout(1150.0, "See exactly what the **room** sees"))
            .into_any_element()
    }

    /// The search types itself, results follow, then a spoken reference is
    /// caught.
    pub fn scripture(&self) -> AnyElement {
        let k = self.k();
        let query = "rom 8 28";
        let typed = self.clock.typed(query.len(), 500.0, 80.0);
        let done = typed == query.len();
        let typed_at = 500.0 + 80.0 * query.len() as f32;
        let results = [
            (
                "Romans 8:28",
                "And we know that all things work together for good…",
            ),
            (
                "Romans 8:29",
                "For whom he did foreknow, he also did predestinate…",
            ),
            (
                "Romans 8:30",
                "Moreover whom he did predestinate, them he also called…",
            ),
        ];
        let after = Clock {
            t: self.clock.t - typed_at,
            ..self.clock
        };
        let search = h_flex()
            .items_center()
            .gap_2()
            .rounded_md()
            .bg(k.surface_tertiary)
            .px(px(10.))
            .py_2()
            .child(
                Icon::new(KairoIcon::Search)
                    .size(px(12.))
                    .text_color(k.text(TextStep::T500)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .text_color(k.text(TextStep::T200))
                    .child(query[..typed].to_owned())
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(12.))
                            .ml(px(1.))
                            .when(self.clock.caret(), |el| el.bg(k.control_accent)),
                    ),
            )
            .child(
                div()
                    .rounded(px(4.))
                    .bg(k.surface_elevated)
                    .px(px(6.))
                    .py(px(2.))
                    .text_size(px(9.))
                    .text_color(k.text(TextStep::T400))
                    .child("KJV"),
            );
        let list = v_flex().min_h(px(150.)).when(done, |el| {
            el.children(results.iter().enumerate().map(|(i, (reference, verse))| {
                after
                    .rise(
                        v_flex()
                            .mt_1()
                            .rounded(px(4.))
                            .px(px(10.))
                            .py_2()
                            .when(i == 0, |el| el.bg(k.surface_elevated))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T200))
                                    .child(*reference),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(k.text(TextStep::T500))
                                    .child(*verse),
                            ),
                        i,
                        0.0,
                    )
                    .into_any_element()
            }))
        });
        let caught = self.clock.ob_in(
            h_flex()
                .items_center()
                .gap_3()
                .rounded_md()
                .bg(k.surface_secondary)
                .px_3()
                .py(px(10.))
                .text_size(px(12.))
                .child(
                    Icon::new(KairoIcon::BookOpen)
                        .size(px(14.))
                        .text_color(k.notice_text),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(k.text(TextStep::T400))
                        .child(rich(
                            "“…turn with me to **Psalm 23**”",
                            k.text(TextStep::T200),
                        )),
                )
                .child(
                    self.clock.pop(
                        div()
                            .rounded(px(4.))
                            .bg(k.surface_tertiary)
                            .px_2()
                            .py(px(2.))
                            .text_color(k.text(TextStep::T300))
                            .child("Psalm 23:1"),
                        0,
                        1900.0,
                    ),
                ),
            0,
            1500.0,
            64.0,
        );
        v_flex()
            .gap(px(18.))
            .child(self.window(
                Some("Scripture"),
                120.0,
                v_flex().px_3().pb_3().child(search).child(list),
            ))
            .child(self.callout(300.0, "Look up a verse **by reference or by words**"))
            .child(caught)
            .child(self.callout(1700.0, "Kairo **hears the reference** and queues it up"))
            .into_any_element()
    }

    /// The song is picked, then its arrangement deals out slide by slide.
    pub fn songs(&self) -> AnyElement {
        let k = self.k();
        let groups = [
            ("Verse 1", 0usize),
            ("Chorus", 1),
            ("Verse 2", 0),
            ("Chorus", 1),
            ("Bridge", 2),
            ("Chorus", 1),
        ];
        let side = self.side().w(relative(0.34)).children(
            [
                "Amazing Grace",
                "Goodness of God",
                "Way Maker",
                "Build My Life",
            ]
            .iter()
            .enumerate()
            .map(|(i, name)| {
                self.row(
                    name,
                    i,
                    300.0,
                    (i == 2).then_some((650.0, k.surface_elevated, k.text(TextStep::T200))),
                )
            }),
        );
        let grid =
            div()
                .flex_1()
                .p(px(10.))
                .pt_0()
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .children(groups.iter().enumerate().map(|(i, (label, color))| {
                            self.clock.pop(
                                v_flex()
                                    .w(relative(0.31))
                                    .overflow_hidden()
                                    .rounded(px(2.))
                                    .child(
                                        div()
                                            .h(px(36.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .bg(self.ob.ink)
                                            .px_1()
                                            .text_center()
                                            .text_size(px(7.))
                                            .text_color(self.ob.slide_text.0)
                                            .child(if i % 2 == 1 {
                                                "Way maker, miracle worker"
                                            } else {
                                                "You are here, moving in our midst"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .px_1()
                                            .py(px(2.))
                                            .text_size(px(8.))
                                            .text_color(self.ob.slide_text.0)
                                            .bg(self.ob.groups[*color])
                                            .child(*label),
                                    ),
                                i,
                                750.0,
                            )
                        })),
                );
        v_flex()
            .gap(px(18.))
            .child(self.callout(260.0, "Every song, **searchable** in your library"))
            .child(self.window(Some("Lyrics"), 120.0, h_flex().child(side).child(grid)))
            .child(self.callout(
                1200.0,
                "Arrange **verses and choruses** the way your team sings them",
            ))
            .into_any_element()
    }

    /// A deck's pages arrive; the current one is outlined as live.
    pub fn documents(&self) -> AnyElement {
        let k = self.k();
        let pages = [
            ("Welcome", 1usize, false),
            ("Walking in Faith", 0, false),
            ("Hebrews 11:1", 2, false),
            ("Testimony", 3, true),
            ("Three Steps", 1, false),
            ("Next Week", 0, false),
        ];
        let body = v_flex()
            .px_3()
            .pb_3()
            .child(
                self.clock.pop(
                    self.slide(self.ob.art[0], 380.0)
                        .mb_2()
                        .px_4()
                        .child(
                            div()
                                .text_size(px(8.))
                                .text_color(self.ob.slide_text.1)
                                .child("PART ONE"),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Walking in Faith"),
                        ),
                    0,
                    400.0,
                ),
            )
            .child(h_flex().gap(px(6.)).children(pages.iter().enumerate().map(
                |(i, (title, bg, video))| {
                    self.clock.pop(
                        div()
                            .relative()
                            .flex_1()
                            .h(px(40.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .px(px(2.))
                            .text_center()
                            .text_size(px(6.))
                            .text_color(self.ob.thumb_text)
                            .bg(self.ob.art[*bg])
                            .when(i == 1, |el| el.border_2().border_color(k.live))
                            .child(*title)
                            .when(*video, |el| {
                                el.child(
                                    div().absolute().bottom(px(2.)).right(px(2.)).child(
                                        Icon::new(KairoIcon::Film)
                                            .size(px(8.))
                                            .text_color(self.ob.thumb_text),
                                    ),
                                )
                            }),
                        i,
                        650.0,
                    )
                },
            )));
        v_flex()
            .gap(px(18.))
            .child(self.callout(260.0, "Bring in **PowerPoint and PDF** as they are"))
            .child(self.window(Some("Documents · Sunday Sermon.pptx"), 120.0, body))
            .child(self.callout(1150.0, "Videos in your deck **play right on the slide**"))
            .into_any_element()
    }

    /// Backgrounds deal into the dock, then a theme restyles the same verse.
    pub fn style(&self) -> AnyElement {
        let backgrounds = [1usize, 2, 0, 3, 4, 5];
        let themes = [
            ("Classic", 6usize, true),
            ("Modern", 0, false),
            ("Lower third", 2, false),
        ];
        let dock = h_flex()
            .gap(px(6.))
            .px_3()
            .pb_3()
            .children(backgrounds.iter().enumerate().map(|(i, bg)| {
                self.clock.pop(
                    div()
                        .relative()
                        .flex_1()
                        .h(px(36.))
                        .rounded(px(2.))
                        .bg(self.ob.art[*bg])
                        .when(i % 3 == 1, |el| {
                            el.child(
                                div().absolute().bottom(px(2.)).right(px(2.)).child(
                                    Icon::new(KairoIcon::Film)
                                        .size(px(8.))
                                        .text_color(self.ob.thumb_text),
                                ),
                            )
                        }),
                    i,
                    350.0,
                )
            }));
        let k = self.k();
        let row =
            h_flex()
                .gap_2()
                .children(themes.iter().enumerate().map(|(i, (name, bg, serif))| {
                    let selected = i == 1;
                    let t = self.clock.select(1250.0);
                    let mut frame = self
                        .slide(self.ob.art[*bg], 150.0)
                        .px_2()
                        .when(i == 2, |el| el.justify_end().pb(px(8.)))
                        .child(
                            div()
                                .text_size(px(9.))
                                .when(*serif, |el| el.font_family("Georgia"))
                                .child("The Lord is my shepherd"),
                        )
                        .child(
                            div()
                                .mt(px(2.))
                                .text_size(px(6.))
                                .text_color(self.ob.slide_text.1)
                                .child("PSALM 23:1"),
                        );
                    if selected {
                        frame = frame.border_2().border_color(self.ob.thumb_text.opacity(t));
                    }
                    self.clock
                        .rise(
                            v_flex().flex_1().child(frame).child(
                                div()
                                    .mt_1()
                                    .text_center()
                                    .text_size(px(10.))
                                    .text_color(k.text(TextStep::T500))
                                    .child(*name),
                            ),
                            i,
                            800.0,
                        )
                        .into_any_element()
                }));
        v_flex()
            .gap(px(18.))
            .child(self.callout(260.0, "Every background and video in **one media dock**"))
            .child(self.window(Some("Media"), 120.0, dock))
            .child(row)
            .child(self.callout(1350.0, "One **theme** restyles every verse and lyric"))
            .into_any_element()
    }

    /// Screens list in, the stage display fills in, and its timer runs.
    pub fn screens(&self) -> AnyElement {
        let k = self.k();
        let screens = [
            ("Screens", "Projector"),
            ("", "Lobby TV"),
            ("Stage", "Stage display"),
        ];
        let side = self
            .side()
            .w(relative(0.38))
            .children(screens.iter().enumerate().map(|(i, (section, name))| {
                let selected = i == 2;
                let t = self.clock.select(700.0);
                self.clock
                    .rise(
                        v_flex()
                            .when(!section.is_empty(), |el| {
                                el.child(
                                    div()
                                        .px(px(6.))
                                        .pb_1()
                                        .when(i > 0, |el| el.mt_2())
                                        .text_size(px(9.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(k.text(TextStep::T500))
                                        .child(section.to_uppercase()),
                                )
                            })
                            .child(
                                v_flex()
                                    .rounded(px(4.))
                                    .px(px(6.))
                                    .py(px(6.))
                                    .when(selected, |el| {
                                        el.bg(k.surface_elevated.opacity(t))
                                            .text_color(k.text(TextStep::T200))
                                    })
                                    .child(*name)
                                    .child(
                                        div()
                                            .text_size(px(9.))
                                            .text_color(k.text(TextStep::T500))
                                            .child("1920 × 1080"),
                                    ),
                            ),
                        i,
                        300.0,
                    )
                    .into_any_element()
            }));
        let stage = div().flex_1().p_3().pt_0().child(
            self.clock.pop(
                self.slide(self.ob.ink, 300.0)
                    .px_3()
                    .child(
                        div()
                            .w_full()
                            .text_size(px(8.))
                            .text_color(k.text(TextStep::T500))
                            .child("CURRENT"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Way maker, miracle worker"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(9.))
                            .text_color(k.text(TextStep::T400))
                            .child("Next: Promise keeper, light in the darkness"),
                    )
                    .child(
                        h_flex()
                            .mt_2()
                            .w_full()
                            .justify_between()
                            .px_1()
                            .font_family(text::MONO_FAMILY)
                            .text_size(px(11.))
                            .child(div().text_color(k.text(TextStep::T300)).child("10:42 AM"))
                            .child(
                                div()
                                    .text_color(k.notice_text)
                                    .child(SharedString::from(self.clock.countdown(299))),
                            ),
                    ),
                0,
                800.0,
            ),
        );
        v_flex()
            .gap(px(18.))
            .child(self.callout(260.0, "Configure each **screen** to match your room"))
            .child(self.window(Some("Screens"), 120.0, h_flex().child(side).child(stage)))
            .child(self.callout(
                1100.0,
                "Give the band a **stage display** with next slide and timer",
            ))
            .into_any_element()
    }

    /// The art for slide `id`.
    pub fn for_slide(&self, id: &str) -> AnyElement {
        match id {
            "present" => self.present(),
            "scripture" => self.scripture(),
            "songs" => self.songs(),
            "documents" => self.documents(),
            "style" => self.style(),
            _ => self.screens(),
        }
    }
}

/// One slide's text and art at `clock`. `width` is the window's, for the
/// headline's `clamp()` sizes.
pub fn render_slide(
    slide: &TourSlide,
    clock: Clock,
    width: f32,
    offset: f32,
    ob: &OnboardingColors,
    cx: &App,
) -> AnyElement {
    let k = cx.kairo();
    let story = clock;
    let clock = Clock::still();
    let headline = (width * 0.06).clamp(44.0, 72.0);
    let subhead = (width * 0.026).clamp(22.0, 30.0);
    let narrow = width <= 820.0;
    let words = v_flex()
        .min_w_0()
        .flex_basis(relative(5.0 / 11.0))
        .child(
            clock.ob_in(
                div()
                    .text_size(gpui_kit::rems(headline / 16.))
                    .line_height(relative(1.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ob.text)
                    .child(slide.headline),
                0,
                0.0,
                28.0,
            ),
        )
        .child(
            clock.ob_in(
                div()
                    .mt(gpui_kit::rems(0.625))
                    .text_size(gpui_kit::rems(subhead / 16.))
                    .line_height(relative(1.15))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(ob.text_deep)
                    .child(slide.subhead),
                1,
                0.0,
                28.0,
            ),
        )
        .child(
            v_flex()
                .mt(gpui_kit::rems(2.25))
                .gap(gpui_kit::rems(0.875))
                .text_size(gpui_kit::rems(1.0625))
                .text_color(k.text(TextStep::T300))
                .children(slide.bullets.iter().enumerate().map(|(i, bullet)| {
                    clock
                        .ob_in(
                            h_flex()
                                .items_start()
                                .gap_3()
                                .child(
                                    div()
                                        .mt(gpui_kit::rems(0.5625))
                                        .size(gpui_kit::rems(0.375))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(ob.blue_soft),
                                )
                                .child(*bullet),
                            i + 3,
                            0.0,
                            28.0,
                        )
                        .into_any_element()
                })),
        );
    let art = Art {
        clock: story,
        ob,
        cx,
    };
    h_flex()
        .size_full()
        .items_center()
        .gap(gpui_kit::rems(3.0))
        .pt(px(if narrow { 32.0 } else { 48.0 }))
        .px(px(if narrow { 28.0 } else { 48.0 }))
        .pb(gpui_kit::rems(1.0))
        .child(words)
        .when(!narrow, |el| {
            el.child(
                div()
                    .relative()
                    .left(px(offset * -24.))
                    .min_w_0()
                    .flex_basis(relative(6.0 / 11.0))
                    .child(art.for_slide(slide.id)),
            )
        })
        .into_any_element()
}

/// Whether `ob::out` is still playing for a slide that left `t` ms ago.
pub fn leaving_visible(t: f32) -> bool {
    t < 320.0
}

/// Re-exported for the stage.
pub use ob::out as slide_out;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_slides_three_bullets_each() {
        assert_eq!(TOUR_SLIDES.len(), 6);
        assert_eq!(TOUR_SLIDES[4].headline, "Themes & Media");
    }

    #[test]
    fn rich_text_marks_bold_runs() {
        let text = rich("Build a **playlist** to set", gpui_kit::black());
        let _ = text;
        let mut plain = String::new();
        for (i, part) in "Build a **playlist** to set".split("**").enumerate() {
            plain.push_str(part);
            let _ = i;
        }
        assert_eq!(plain, "Build a playlist to set");
    }
}
