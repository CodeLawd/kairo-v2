//! Writes a sample deck as a .pptx and its slides as PNGs, for opening in
//! PowerPoint, Keynote or LibreOffice:
//! `cargo run -p kairo-imagine --example pptx_sample -- out.pptx`.

use kairo_imagine::deck::{
    CardStyle, Content, DeckTheme, Design, Layout, Ornament, Point, Shape, ShapeKind, TitleAlign,
    compose, render,
};
use kairo_imagine::pptx::{ExportSlide, write};

fn point(label: &str, heading: &str, text: &str, value: Option<f64>) -> Point {
    Point {
        label: label.into(),
        heading: heading.into(),
        text: text.into(),
        value,
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("an output path");
    // `bold` as the second argument draws a second design direction.
    let theme = if std::env::args().nth(2).as_deref() == Some("bold") {
        DeckTheme {
            background: "#101418".into(),
            surface: Some("#1B222A".into()),
            text: "#F4F1EA".into(),
            muted: Some("#A9B4BE".into()),
            accent: "#FF6B4A".into(),
            accent_soft: Some("#3A2A28".into()),
            title_background: Some("#FF6B4A".into()),
            title_text: Some("#101418".into()),
            heading_font: "Futura".into(),
            body_font: "Avenir Next".into(),
            style: "bold graphic".into(),
            design: Design {
                ornament: Ornament::Blobs,
                cards: CardStyle::AccentTop,
                radius: 4.,
                stripe: false,
                align: TitleAlign::Center,
                uppercase_titles: true,
                ..Design::default()
            },
        }
    } else {
        DeckTheme::default()
    };
    let deck = "AI Tools for the Church";
    let slides = vec![
        Content {
            layout: Layout::Title,
            eyebrow: "A practical guide for ministry teams".into(),
            title: deck.into(),
            lead: "Practical help without losing the heart of ministry".into(),
            callout: "AI can support routine work, but never replace prayer or pastoral care."
                .into(),
            ..Content::default()
        },
        Content {
            layout: Layout::Cards,
            eyebrow: "Where AI can help".into(),
            title: "Sermon & Teaching Preparation".into(),
            lead: "Use AI to support preparation, not to replace Scripture study.".into(),
            points: vec![
                point(
                    "",
                    "Explore sermon series",
                    "Brainstorm themes, titles and teaching angles.",
                    None,
                ),
                point(
                    "",
                    "Organize study notes",
                    "Turn scattered outlines into clear structures.",
                    None,
                ),
                point(
                    "",
                    "Draft discussion questions",
                    "Prepare prompts for deeper conversation.",
                    None,
                ),
                point(
                    "",
                    "Adapt group guides",
                    "Create youth and small-group drafts.",
                    None,
                ),
            ],
            ..Content::default()
        },
        Content {
            layout: Layout::Checklist,
            eyebrow: "Keep people and truth first".into(),
            title: "Wisdom, Guardrails & Ethics".into(),
            lead: "Every AI draft needs human judgment, care and accountability.".into(),
            points: vec![
                point(
                    "",
                    "Check theological accuracy",
                    "Verify Scripture references and claims.",
                    None,
                ),
                point(
                    "",
                    "Protect personal privacy",
                    "Never upload member or children's details.",
                    None,
                ),
                point(
                    "",
                    "Keep pastoral matters private",
                    "Never share counselling or prayer requests.",
                    None,
                ),
                point(
                    "",
                    "Preserve your church's voice",
                    "Edit every draft for warmth and care.",
                    None,
                ),
            ],
            ..Content::default()
        },
        Content {
            layout: Layout::Stats,
            eyebrow: "Time back for people".into(),
            title: "What Teams Report".into(),
            lead: "Small, routine tasks add up across a week.".into(),
            points: vec![
                point(
                    "4 hrs",
                    "saved weekly",
                    "On bulletins, emails and run sheets.",
                    None,
                ),
                point(
                    "3x",
                    "faster drafts",
                    "For announcements and captions.",
                    None,
                ),
                point(
                    "100%",
                    "human reviewed",
                    "Nothing goes out unchecked.",
                    None,
                ),
            ],
            ..Content::default()
        },
        Content {
            layout: Layout::Chart,
            eyebrow: "Where the hours go".into(),
            title: "Weekly Admin Time".into(),
            lead: "Hours per week before using AI tools.".into(),
            points: vec![
                point("", "Bulletins", "", Some(5.)),
                point("", "Emails", "", Some(7.)),
                point("", "Slides", "", Some(4.)),
                point("", "Social posts", "", Some(3.)),
            ],
            ..Content::default()
        },
        Content {
            layout: Layout::Steps,
            eyebrow: "A simple plan for this month".into(),
            title: "Your Team's Next Steps".into(),
            lead: "Start small, learn together and measure what helps.".into(),
            points: vec![
                point(
                    "Week 1",
                    "Test one routine task",
                    "Choose a bulletin or email and compare time saved.",
                    None,
                ),
                point(
                    "Week 2",
                    "Agree on guidelines",
                    "Set rules for review and what not to share.",
                    None,
                ),
                point(
                    "Weeks 3-4",
                    "Review together",
                    "Discuss what helped and what stays human-led.",
                    None,
                ),
            ],
            callout: "The goal is more time and attention for people.".into(),
            ..Content::default()
        },
        Content {
            layout: Layout::Quote,
            title: "Psalm 96:1".into(),
            lead: "Sing to the Lord a new song.".into(),
            ..Content::default()
        },
    ];
    let pages = slides.len();
    let dir = std::path::Path::new(&out).with_extension("");
    std::fs::create_dir_all(&dir).expect("png dir");
    let export: Vec<ExportSlide> = slides
        .into_iter()
        .enumerate()
        .map(|(i, mut c)| {
            c.page = i + 1;
            c.pages = pages;
            c.deck = deck.into();
            if i == 1 {
                c.shapes = vec![Shape {
                    kind: ShapeKind::Circle,
                    x: 88.,
                    y: -12.,
                    w: 18.,
                    h: 32.,
                    color: "accent".into(),
                    opacity: 0.25,
                    radius: 0.,
                }];
            }
            std::fs::write(
                dir.join(format!("{i}.png")),
                render(&theme, &c, None, None).expect("png"),
            )
            .expect("write png");
            let (page, marks) = compose(&theme, &c);
            ExportSlide {
                page,
                marks,
                background: None,
                art: None,
                notes: format!("Notes for {}.", c.title),
            }
        })
        .collect();
    std::fs::write(&out, write(deck, &export).expect("pptx")).expect("write");
    println!("{out}");
}
