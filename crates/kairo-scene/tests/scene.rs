//! Scene building, text safety, fonts, fit and the raster pool.

use kairo_domain::overlay::*;
use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
use kairo_scene::color::Rgba;
use kairo_scene::*;
use std::sync::mpsc;
use std::time::Duration;

const FRAME: (u32, u32) = (1920, 1080);

fn theme() -> OverlayTheme {
    DEFAULT_OVERLAY_THEME.clone()
}

fn scene(theme: &OverlayTheme, reference: &str, text: &str) -> SlideScene {
    SlideScene::build(theme, reference, text, FRAME, &[])
}

#[test]
fn blank_text_and_hidden_or_blank_references_leave_no_box() {
    let t = theme();
    assert_eq!(scene(&t, "John 3:16", "").boxes.len(), 1);
    assert_eq!(
        scene(&t, "John 3:16", " \n\t ").boxes[0].kind,
        BoxKind::Reference
    );
    assert!(scene(&t, "", "").boxes.is_empty());
    assert!(scene(&t, " \u{a0} ", "\u{3000}").boxes.is_empty());
    let mut hidden = theme();
    hidden.reference.show = false;
    let s = scene(&hidden, "John 3:16", "For God");
    assert_eq!(s.boxes.len(), 1);
    assert_eq!(s.boxes[0].kind, BoxKind::Verse);
    // Verse first, then the reference, as in the template.
    let both = scene(&t, "John 3:16", "For God");
    assert_eq!(
        both.boxes.iter().map(|b| b.kind).collect::<Vec<_>>(),
        [BoxKind::Verse, BoxKind::Reference]
    );
    // A scene with nothing to show draws nothing.
    assert!(rasterize(&scene(&t, "", "")).is_none());
}

#[test]
fn untrusted_text_cannot_change_styling() {
    let t = theme();
    let clean = scene(&t, "Ref", "plain");
    let hostile_text = r##"</div><span style="color:red;font-size:900px">x</span> &amp; "quoted" 'single' <img src=x onerror=alert(1)>"##;
    let hostile = scene(&t, "<b style=\"color:#f00\">Ref</b>", hostile_text);
    for (a, b) in clean.boxes.iter().zip(&hostile.boxes) {
        assert_eq!(a.style, b.style, "style of {:?}", a.kind);
        assert_eq!(
            (a.rect, a.padding, a.vertical, a.size),
            (b.rect, b.padding, b.vertical, b.size)
        );
    }
    // The text is literal: markup stays as characters.
    assert_eq!(hostile.boxes[0].paragraphs.len(), 1);
    assert!(
        hostile.boxes[0].paragraphs[0]
            .text
            .contains("<span style=\"color:red;font-size:900px\">")
    );
    assert!(
        hostile.boxes[0]
            .paragraphs
            .iter()
            .all(|p| p.color.is_none())
    );
    // And it lays out as text: the line break check sees the tags as words.
    let laid = analyse(&hostile);
    assert!(laid[0].lines.join(" ").contains("onerror=alert(1)>"));
    // Only a sanitised hex colour can come from a line.
    let lines: Vec<ColoredLine> = [
        "#fff",
        "#5EEAD4",
        "red",
        "#12",
        "#fff;background:url(x)",
        "rgb(1,2,3)",
        "  #abcdef  ",
    ]
    .iter()
    .map(|c| ColoredLine {
        text: "line".into(),
        color: Some((*c).into()),
    })
    .collect();
    let s = SlideScene::build(&t, "", "x", FRAME, &lines);
    let colors: Vec<_> = s.boxes[0].paragraphs.iter().map(|p| p.color).collect();
    assert_eq!(
        colors,
        [
            Some(Rgba::new(255, 255, 255, 255)),
            Some(Rgba::new(0x5E, 0xEA, 0xD4, 255)),
            None,
            None,
            None,
            None,
            Some(Rgba::new(0xAB, 0xCD, 0xEF, 255)),
        ]
    );
}

#[test]
fn coloured_lines_replace_the_verse_body_only_when_present() {
    let t = theme();
    let none = SlideScene::build(&t, "", "from text", FRAME, &[]);
    assert_eq!(none.boxes[0].paragraphs[0].text, "from text");
    let some = SlideScene::build(
        &t,
        "",
        "from text",
        FRAME,
        &[ColoredLine {
            text: "from lines".into(),
            color: None,
        }],
    );
    assert_eq!(some.boxes[0].paragraphs[0].text, "from lines");
    // The body is still gated on `text`.
    assert!(
        SlideScene::build(
            &t,
            "",
            "  ",
            FRAME,
            &[ColoredLine {
                text: "x".into(),
                color: None
            }]
        )
        .boxes
        .is_empty()
    );
    // The JSON form: any entry without a string text fails the template.
    let ok = serde_json::json!([{"text":"a","color":"#fff"},{"text":"b"}]);
    assert_eq!(colored_lines_from_json(&ok).unwrap().len(), 2);
    assert!(colored_lines_from_json(&serde_json::json!([{"color":"#fff"}])).is_none());
    assert!(colored_lines_from_json(&serde_json::json!("nope")).is_none());
}

#[test]
fn stroke_and_shadow_follow_outline_and_shadow_css() {
    let mut t = theme();
    t.verse.outline = OverlayTextOutline {
        enabled: true,
        position: OverlayOutlinePosition::Inside,
        width_px: 4.0,
        style: OverlayOutlineStyle::Dashed,
        color: "#102030".into(),
        opacity: 0.5,
        extra: Default::default(),
    };
    let stroke = |t: &OverlayTheme| scene(t, "", "x").boxes[0].style.stroke;
    // Inside is 0.55 of the width; dashed falls back to solid.
    assert_eq!(stroke(&t).unwrap().width, 4.0 * 0.55);
    assert_eq!(stroke(&t).unwrap().color, Rgba::new(0x10, 0x20, 0x30, 128));
    t.verse.outline.width_px = 0.5;
    assert_eq!(
        stroke(&t).unwrap().width,
        0.5,
        "inside keeps at least half a pixel"
    );
    t.verse.outline.position = OverlayOutlinePosition::Center;
    t.verse.outline.style = OverlayOutlineStyle::Dotted;
    assert_eq!(stroke(&t).unwrap().width, 0.5);
    t.verse.outline.width_px = 0.0;
    assert!(stroke(&t).is_none());
    t.verse.outline.width_px = 3.0;
    t.verse.outline.enabled = false;
    assert!(stroke(&t).is_none());
    t.verse.outline.enabled = true;
    t.verse.outline.color = "not a colour".into();
    assert!(
        stroke(&t).is_none(),
        "an unparsable colour drops the declaration"
    );

    let mut t = theme();
    t.verse.shadow = OverlayTextShadow {
        enabled: true,
        x_px: 1.0,
        y_px: 2.0,
        blur_px: 6.0,
        color: "rgba(1, 2, 3, 0.9)".into(),
        opacity: 0.5,
        extra: Default::default(),
    };
    let sh = scene(&t, "", "x").boxes[0].style.shadow.unwrap();
    assert_eq!(
        (sh.x, sh.y, sh.blur, sh.color),
        (1.0, 2.0, 6.0, Rgba::new(1, 2, 3, 128))
    );
    t.verse.shadow.enabled = false;
    assert!(scene(&t, "", "x").boxes[0].style.shadow.is_none());
}

#[test]
fn backdrop_and_colours_with_opacity() {
    let mut t = theme();
    t.layout.backdrop_box = true;
    t.layout.backdrop_color = "rgba(3, 10, 20, 0.75)".into();
    t.verse.color = "#ffffff".into();
    t.verse.color_opacity = 0.5;
    let s = scene(&t, "Ref", "x");
    assert_eq!(s.boxes[0].backdrop, Some(Rgba::new(3, 10, 20, 191)));
    assert_eq!(
        s.boxes[1].backdrop, s.boxes[0].backdrop,
        "both boxes carry the backdrop"
    );
    assert_eq!(s.boxes[0].style.color, Rgba::new(255, 255, 255, 128));
    t.layout.backdrop_box = false;
    assert!(scene(&t, "Ref", "x").boxes[0].backdrop.is_none());
    // An unparsable text colour falls back to the document's black.
    t.verse.color = "nonsense".into();
    assert_eq!(scene(&t, "", "x").boxes[0].style.color, Rgba::BLACK);
}

#[test]
fn rasters_are_premultiplied_deterministic_and_bounded_by_the_boxes() {
    let mut t = theme();
    t.layout.backdrop_box = true;
    let s = scene(&t, "John 3:16", "For God so loved the world");
    let a = rasterize(&s).expect("draws");
    let b = rasterize(&s).expect("draws");
    assert_eq!(a, b);
    assert_eq!(a.pixels.len(), (a.width * a.height * 4) as usize);
    assert!(
        a.pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3])
    );
    assert!(a.x + a.width <= FRAME.0 && a.y + a.height <= FRAME.1);
    // Something opaque-ish from the backdrop, something transparent outside the boxes.
    assert!(a.pixels.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
}

#[test]
fn fit_stays_between_twelve_and_the_cap_and_shrinks_for_long_text() {
    let mut t = theme();
    t.layout.auto_fit_text = true;
    let size = |text: &str| analyse(&scene(&t, "", text))[0].size;
    let short = size("Jesus wept.");
    let verse = size(fit::SMART_FILL_REFERENCE);
    assert_eq!(short, verse, "short lines stay verse-sized (smart fill)");
    let long = size(&"word ".repeat(400));
    assert!(long < verse && long >= 12.0, "{long}");
    // Even when nothing fits the floor is 12.
    let mut tiny = theme();
    tiny.layout.auto_fit_text = true;
    tiny.verse.frame.height_pct = 6.0;
    assert_eq!(
        analyse(&scene(&tiny, "", &"word ".repeat(2000)))[0].size,
        12.0
    );
    // A fixed size is used as is.
    let mut fixed = theme();
    fixed.layout.auto_fit_text = false;
    fixed.verse.font_size_px = 33.0;
    assert_eq!(analyse(&scene(&fixed, "", "x"))[0].size, 33.0);
}

#[test]
fn an_unbreakable_word_breaks_and_blank_paragraphs_keep_their_height() {
    let mut t = theme();
    t.layout.auto_fit_text = false;
    t.verse.font_size_px = 60.0;
    let lines = &analyse(&scene(&t, "", &"x".repeat(300)))[0].lines;
    assert!(lines.len() > 1 && lines.iter().all(|l| !l.is_empty()));
    let one = scene(&t, "", "a\nb");
    let two = scene(&t, "", "a\n\nb");
    let (h1, h2) = kairo_scene::with_typesetter(|ts| {
        let w = one.boxes[0].rect.width - 2.0 * one.boxes[0].padding;
        (
            ts.shape(&one.boxes[0], 60.0, w, false).height,
            ts.shape(&two.boxes[0], 60.0, w, false).height,
        )
    });
    assert!(
        (h2 - h1 - 60.0 * t.verse.line_height as f32).abs() < 0.01,
        "a blank line is one line tall: {h1} {h2}"
    );
    // A trailing newline adds no line.
    let trailing = scene(&t, "", "a\nb\n");
    let h3 = kairo_scene::with_typesetter(|ts| {
        let w = trailing.boxes[0].rect.width - 2.0 * trailing.boxes[0].padding;
        ts.shape(&trailing.boxes[0], 60.0, w, false).height
    });
    assert_eq!(h1, h3);
}

#[test]
fn cursive_scripts_ignore_letter_spacing_and_latin_does_not() {
    let width = |text: &str, spacing: f64| {
        let mut t = theme();
        t.layout.auto_fit_text = false;
        t.verse.letter_spacing_px = spacing;
        let s = scene(&t, "", text);
        kairo_scene::with_typesetter(|ts| {
            ts.shape(&s.boxes[0], 48.0, 5000.0, false).lines[0].advance
        })
    };
    let arabic = "الرَّبُّ رَاعِيَّ";
    assert!((width(arabic, 0.0) - width(arabic, -3.0)).abs() < 0.01);
    assert!(width("shepherd", 0.0) - width("shepherd", -3.0) > 10.0);
}

#[test]
fn yoruba_and_the_other_bundled_scripts_have_a_bundled_face() {
    for c in "ẹọṣẸỌṢàáèéìíòóùúńǹ"
        .chars()
        .chain("ẹ́ọ̀ṣ́".chars())
        .chain("ΑΒΓαβγ".chars())
        .chain("АБВабв".chars())
        .chain("ÀẢỆ".chars())
    {
        assert!(bundled_font_covers(c), "{c:?} U+{:04X}", c as u32);
    }
    assert!(!bundled_font_covers('\u{4e2d}'));
}

#[test]
fn every_picker_stack_and_odd_older_stacks_resolve_with_the_bundled_fallback() {
    for (_, css) in FONT_STACKS {
        let chain = family_chain(css);
        assert!(
            chain.len() > css.split(',').count() + bundled_families().count(),
            "{css}"
        );
    }
    // The bundled families and a system generic always end the chain.
    let chain = family_chain("'Some Missing Font', Arial");
    let names: Vec<String> = chain.iter().map(|f| f.to_string()).collect();
    assert!(
        names.iter().any(|n| n.contains("Source Sans 3")),
        "{names:?}"
    );
    // CSS that cannot parse is a dropped declaration: the browser default, a serif, first.
    assert!(
        family_chain("'unterminated")[0]
            .to_string()
            .contains("serif")
    );
    assert!(family_chain("")[0].to_string().contains("serif"));
    // A missing family still shapes and draws.
    let mut t = theme();
    t.verse.font_family = "Definitely Not Installed, nonsense".into();
    assert!(rasterize(&scene(&t, "", "Ìjà dópin, ogun sì tán")).is_some());
}

#[test]
fn equal_scenes_hash_equal_and_any_change_changes_the_hash() {
    let t = theme();
    let base = scene(&t, "John 3:16", "For God so loved").content_hash();
    assert_eq!(
        base,
        scene(&t, "John 3:16", "For God so loved").content_hash()
    );
    assert_ne!(
        base,
        scene(&t, "John 3:17", "For God so loved").content_hash()
    );
    assert_ne!(
        base,
        scene(&t, "John 3:16", "For God so loved!").content_hash()
    );
    let mut c = theme();
    c.verse.color = "#fe0000".into();
    assert_ne!(
        base,
        scene(&c, "John 3:16", "For God so loved").content_hash()
    );
    let mut c = theme();
    c.verse.shadow.blur_px += 1.0;
    assert_ne!(
        base,
        scene(&c, "John 3:16", "For God so loved").content_hash()
    );
    let mut c = theme();
    c.verse.frame.x_pct += 1.0;
    assert_ne!(
        base,
        scene(&c, "John 3:16", "For God so loved").content_hash()
    );
    assert_ne!(
        base,
        SlideScene::build(&t, "John 3:16", "For God so loved", (1920, 1440), &[]).content_hash()
    );
}

#[test]
fn the_pool_builds_off_thread_caches_by_hash_and_survives_a_failing_job() {
    let pool: RasterPool<Option<std::sync::Arc<TextRaster>>> =
        RasterPool::new(2, 64 << 20, |r| r.map(std::sync::Arc::new));
    let t = theme();
    let s = scene(&t, "John 3:16", "For God so loved the world");
    let (tx, rx) = mpsc::channel();
    let submit = |s: SlideScene, tx: mpsc::Sender<_>| {
        pool.submit(Box::new(move || Some(s)), move |d| tx.send(d).unwrap());
    };
    submit(s.clone(), tx.clone());
    let first = rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(!first.cached);
    assert_eq!(first.hash, s.content_hash());
    let drawn = first.value.clone().unwrap().expect("drew");
    assert_eq!(Some((*drawn).clone()), rasterize(&s));
    submit(s.clone(), tx.clone());
    let second = rx.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(second.cached);
    assert!(
        std::sync::Arc::ptr_eq(&drawn, &second.value.unwrap().unwrap()),
        "a hit hands back the same pixels"
    );
    // A job that builds nothing and one that panics both answer, and the pool still works.
    pool.submit(Box::new(|| None), {
        let tx = tx.clone();
        move |d| tx.send(d).unwrap()
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(20))
            .unwrap()
            .value
            .is_none()
    );
    pool.submit(Box::new(|| panic!("scene builder failed")), {
        let tx = tx.clone();
        move |d| tx.send(d).unwrap()
    });
    assert!(
        rx.recv_timeout(Duration::from_secs(20))
            .unwrap()
            .value
            .is_none()
    );
    submit(scene(&t, "", "still alive"), tx);
    assert!(
        rx.recv_timeout(Duration::from_secs(20))
            .unwrap()
            .value
            .is_some()
    );
    // The calling thread can draw through the same cache (thumbnails).
    let (v, done) = pool.render(&s);
    assert!(done.cached);
    assert!(v.is_some());
    assert_eq!(pool.cached().0, 2);
}

#[test]
fn the_cache_respects_its_byte_budget_but_keeps_the_newest() {
    let pool: RasterPool<usize> = RasterPool::new(1, 1, |r| r.map_or(0, |r| r.pixels.len()));
    let t = theme();
    let (a, _) = pool.render(&scene(&t, "", "first"));
    let (b, _) = pool.render(&scene(&t, "", "second one"));
    assert!(a > 0 && b > 0);
    assert_eq!(pool.cached().0, 1);
}

#[test]
fn degenerate_numbers_and_boxes_draw_something_sane_or_nothing_and_never_panic() {
    let mut t = theme();
    t.verse.font_size_px = f64::NAN;
    t.verse.line_height = f64::INFINITY;
    t.verse.letter_spacing_px = f64::NAN;
    t.verse.font_weight = -5.0;
    t.verse.shadow.enabled = true;
    t.verse.shadow.blur_px = f64::NAN;
    t.verse.shadow.x_px = f64::INFINITY;
    t.layout.padding_px = 5000.0;
    t.layout.auto_fit_text = false;
    let _ = rasterize(&scene(&t, "Ref", "text that cannot be laid out sensibly"));
    // A zero-sized box and a box far outside the frame.
    let mut t = theme();
    t.verse.frame.width_pct = 0.0;
    t.verse.frame.x_pct = 400.0;
    t.reference.text.frame.height_pct = 0.0;
    assert!(rasterize(&scene(&t, "Ref", "text")).is_none());
    // A huge size and a huge text.
    let mut t = theme();
    t.layout.auto_fit_text = false;
    t.verse.font_size_px = 1.0e9;
    let _ = rasterize(&scene(&t, "", "big"));
    let mut t = theme();
    t.layout.auto_fit_text = true;
    let long = "In the beginning was the Word. ".repeat(600);
    assert!(rasterize(&scene(&t, "", &long)).is_some());
    // Controls and unusual characters.
    let _ = rasterize(&scene(
        &theme(),
        "\u{0}\u{7}\u{200e}\u{202e}",
        "a\u{0}b\u{7f}\u{202e}c \u{1F600} \u{fffd}",
    ));
}
