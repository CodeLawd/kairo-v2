//! Draws one slide of every layout archetype as PNGs, with a stand-in
//! picture: `cargo run -p kairo-imagine --example archetypes -- out_dir`.

use kairo_imagine::deck::{Content, DeckTheme, Layout, Point, Side, render};

fn point(label: &str, heading: &str, text: &str, value: Option<f64>) -> Point {
    Point {
        label: label.into(),
        heading: heading.into(),
        text: text.into(),
        value,
    }
}

/// A soft diagonal gradient standing in for a painted picture.
fn picture() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(768, 432, |x, y| {
        let t = (x as f32 / 768. + y as f32 / 432.) / 2.;
        image::Rgba([
            (40. + 160. * t) as u8,
            (70. + 90. * t) as u8,
            (110. - 40. * t) as u8,
            255,
        ])
    });
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("an output directory"));
    std::fs::create_dir_all(&dir).expect("dir");
    let theme = DeckTheme::default();
    let art = picture();
    let base = |layout, title: &str| Content {
        layout,
        title: title.into(),
        deck: "AI Tools for the Church".into(),
        pages: 9,
        ..Content::default()
    };
    let slides = vec![
        Content {
            eyebrow: "The real question".into(),
            lead: "Tools change. Calling doesn't.".into(),
            ..base(Layout::Hero, "AI should buy back time for people")
        },
        Content {
            eyebrow: "Where AI helps most".into(),
            lead: "One big win, three quiet ones.".into(),
            points: vec![
                point(
                    "4 hrs",
                    "Weekly admin, reclaimed",
                    "Bulletins, emails and run sheets drafted in minutes.",
                    None,
                ),
                point(
                    "",
                    "Sermon prep",
                    "Outlines and discussion questions.",
                    None,
                ),
                point("", "Media", "Captions and slide backgrounds.", None),
                point("", "Care", "Follow-up reminders, never replies.", None),
            ],
            ..base(Layout::Bento, "Small tasks add up to whole days")
        },
        Content {
            eyebrow: "What teams report".into(),
            points: vec![
                point("4 hrs", "Saved weekly", "Across admin and media.", None),
                point("3x", "Faster drafts", "For announcements.", None),
                point(
                    "100%",
                    "Human reviewed",
                    "Nothing goes out unchecked.",
                    None,
                ),
            ],
            ..base(Layout::Kpi, "The time comes back quickly")
        },
        Content {
            eyebrow: "A simple month".into(),
            points: vec![
                point("Week 1", "Try one task", "A bulletin or an email.", None),
                point(
                    "Week 2",
                    "Set guidelines",
                    "Review and privacy rules.",
                    None,
                ),
                point("Week 3", "Widen it", "Slides and captions.", None),
                point("Week 4", "Review together", "Keep what helped.", None),
            ],
            ..base(Layout::Timeline, "Start small and learn together")
        },
        Content {
            eyebrow: "The difference".into(),
            side: Side::Right,
            points: vec![
                point(
                    "Before",
                    "Sunday prep eats Saturday",
                    "Slides, notices and emails pile up late.",
                    None,
                ),
                point(
                    "After",
                    "Saturday belongs to people",
                    "Drafts arrive early; the team edits and prays.",
                    None,
                ),
            ],
            ..base(Layout::Split, "Same team, different week")
        },
        Content {
            eyebrow: "Worship media".into(),
            lead: "Backgrounds that serve the song, not compete with it.".into(),
            art: true,
            side: Side::Left,
            points: vec![
                point(
                    "",
                    "Calm by default",
                    "Soft light, little detail behind words.",
                    None,
                ),
                point(
                    "",
                    "One look per series",
                    "A locked style for the season.",
                    None,
                ),
            ],
            ..base(Layout::Split, "Let the room focus on worship")
        },
        Content {
            eyebrow: "Vision".into(),
            lead: "More time at the door, less at the desk.".into(),
            art: true,
            side: Side::Right,
            points: vec![point("", "Every Sunday", "Faces before files.", None)],
            ..base(Layout::FullBleed, "Technology that steps aside")
        },
        Content {
            eyebrow: "Across the week".into(),
            lead: "Where the hours go now.".into(),
            points: vec![
                point("Mon", "Plan", "Service outline.", None),
                point("Tue", "Draft", "Notices and emails.", None),
                point("Wed", "Design", "Slides and media.", None),
                point("Thu", "Review", "Theology and tone.", None),
            ],
            ..base(Layout::Panoramic, "A week with room to breathe")
        },
        Content {
            eyebrow: "Guardrails".into(),
            points: vec![
                point(
                    "",
                    "Check every claim",
                    "Scripture and theology stay human.",
                    None,
                ),
                point(
                    "",
                    "Protect people",
                    "No member or child details, ever.",
                    None,
                ),
                point(
                    "",
                    "Keep your voice",
                    "Edit drafts until they sound like you.",
                    None,
                ),
            ],
            ..base(Layout::Editorial, "Wisdom sets the limits")
        },
    ];
    for (i, mut c) in slides.into_iter().enumerate() {
        c.page = i + 1;
        let png = render(&theme, &c, None, Some(&art)).expect("render");
        std::fs::write(dir.join(format!("{i}.png")), png).expect("write");
    }
    println!("{}", dir.display());
}
