//! The editor's rules that need no drawing: which theme opens first, sample
//! slides, the box a picture gets, and the numbers on the status bar.

use std::sync::atomic::{AtomicU64, Ordering};

use kairo_domain::json::to_js_value;
use kairo_domain::overlay::{
    CustomOverlayTheme, OverlayBox, OverlayContentKind, OverlayElement, OverlayElementKind,
    OverlayOutput, OverlayTheme,
};
use kairo_domain::overlay_boxes::{ELEMENT_BOX_MIN, clamp_overlay_box, overlay_box};
use kairo_domain::overlay_defaults::normalize_overlay_theme;
use kairo_domain::overlay_elements::{create_overlay_element, overlay_element_label};
use kairo_domain::overlay_outputs::primary_rendered_output;
use kairo_domain::theme_library::{theme_baseline, themes_for_kind};

use crate::backend::Sample;

/// Edits are saved this long after the last one.
pub const SAVE_DELAY_MS: u64 = 400;
/// "Saved" shows this long.
pub const SAVED_FLASH_MS: u64 = 1200;
/// Output pixels, for the status bar.
pub const OUTPUT_SIZE: (f64, f64) = (1920.0, 1080.0);

const SAMPLE_REFERENCE: &str = "John 3:16 (KJV)";
const SAMPLE_TEXT: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
const SAMPLE_TEXT_LONG: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life. For God sent not his Son into the world to condemn the world; but that the world through him might be saved. He that believeth on him is not condemned: but he that believeth not is condemned already, because he hath not believed in the name of the only begotten Son of God.";
// Lyrics push a slide, not a passage: the reference line is the section label
// and the body is a couplet, so previewing one against the verse sample would
// flatter a theme that is actually far too small on stage.
const SAMPLE_LYRIC_REFERENCE: &str = "Amazing Grace — Verse 1";
const SAMPLE_LYRIC_TEXT: &str = "Amazing grace, how sweet the sound\nThat saved a wretch like me";
const SAMPLE_LYRIC_TEXT_LONG: &str = "Amazing grace, how sweet the sound\nThat saved a wretch like me\nI once was lost, but now am found\nWas blind, but now I see";

/// The slide the preview shows for `kind`.
pub fn sample(kind: OverlayContentKind, long: bool) -> Sample {
    let (reference, short, full) = match kind {
        OverlayContentKind::Scripture => (SAMPLE_REFERENCE, SAMPLE_TEXT, SAMPLE_TEXT_LONG),
        OverlayContentKind::Lyrics => (
            SAMPLE_LYRIC_REFERENCE,
            SAMPLE_LYRIC_TEXT,
            SAMPLE_LYRIC_TEXT_LONG,
        ),
    };
    Sample {
        reference: reference.into(),
        text: if long { full } else { short }.into(),
    }
}

/// The theme as the editor works on it (`normalizeOverlayTheme`), so an
/// older theme's missing fields are filled in.
pub fn normalized(theme: &OverlayTheme) -> OverlayTheme {
    match to_js_value(theme) {
        Ok(raw) => normalize_overlay_theme(Some(&raw)),
        Err(_) => theme.clone(),
    }
}

/// Which theme opens first: the one the editor showed last, else the one
/// the primary screen uses, else the first scripture theme, else the first.
pub fn initial_selection(
    library: &[CustomOverlayTheme],
    outputs: &[OverlayOutput],
    remembered: Option<&str>,
) -> Option<String> {
    let remembered = remembered.and_then(|id| library.iter().find(|t| t.id == id));
    let live = primary_rendered_output(outputs)
        .and_then(|o| o.theme_id.as_deref())
        .and_then(|id| library.iter().find(|t| t.id == id));
    remembered
        .or(live)
        .or_else(|| {
            themes_for_kind(library, OverlayContentKind::Scripture)
                .into_iter()
                .next()
        })
        .or(library.first())
        .map(|t| t.id.clone())
}

/// No edits since the reset point (the bottom "Save theme" button).
pub fn is_default(draft: &OverlayTheme, entry: &CustomOverlayTheme) -> bool {
    *draft == normalized(theme_baseline(entry))
}

/// The widest 16:9 frame that fits the stage both ways, so the canvas fills
/// whatever room the window gives it instead of only following its width.
pub fn fit_frame_width(stage: (f32, f32), padding: (f32, f32, f32, f32)) -> f32 {
    let (left, top, right, bottom) = padding;
    let w = stage.0 - left - right;
    let h = stage.1 - top - bottom;
    (w.min(h * 16.0 / 9.0)).floor().max(0.0)
}

/// A box for newly placed media at the file's own shape: 30 % of the frame
/// wide, centred, shrunk to fit if it is tall. The default box if the file
/// could not be measured.
pub fn media_box(aspect: Option<f64>, kind: OverlayElementKind) -> OverlayBox {
    let Some(aspect) = aspect.filter(|a| a.is_finite() && *a > 0.0) else {
        return create_overlay_element(kind, None, "", None).frame;
    };
    let mut width = 30.0;
    let mut height = (width / 100.0) * OUTPUT_SIZE.0 / aspect / OUTPUT_SIZE.1 * 100.0;
    if height > 70.0 {
        width *= 70.0 / height;
        height = 70.0;
    }
    clamp_overlay_box(
        &overlay_box(50.0 - width / 2.0, 50.0 - height / 2.0, width, height),
        ELEMENT_BOX_MIN,
    )
}

/// A row in the layers list: a picture or video shows its file name.
pub fn element_name(el: &OverlayElement) -> String {
    el.media_path
        .as_deref()
        .and_then(|p| p.rsplit(['/', '\\']).next())
        .filter(|n| !n.is_empty())
        .map_or_else(
            || overlay_element_label(el.kind, el.shape.as_deref()).to_owned(),
            str::to_owned,
        )
}

/// The status bar's X, Y, W and H in output pixels.
pub fn frame_readout(b: &OverlayBox) -> [(&'static str, i64); 4] {
    let round = |v: f64| kairo_domain::js::math_round(v) as i64;
    [
        ("X", round(b.x_pct / 100.0 * OUTPUT_SIZE.0)),
        ("Y", round(b.y_pct / 100.0 * OUTPUT_SIZE.1)),
        ("W", round(b.width_pct / 100.0 * OUTPUT_SIZE.0)),
        ("H", round(b.height_pct / 100.0 * OUTPUT_SIZE.1)),
    ]
}

/// A fresh id for a theme or an element.
pub fn new_id(prefix: &str, now_ms: f64) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let salt = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    format!(
        "{prefix}-{}-{:x}{:x}",
        now_ms as u64,
        std::process::id(),
        n ^ u64::from(salt)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_defaults::{DEFAULT_OVERLAY_THEME, make_overlay_output};
    use kairo_domain::theme_library::{assign_theme_to_output, create_custom_theme};

    fn lib() -> Vec<CustomOverlayTheme> {
        vec![
            create_custom_theme(
                "L",
                &DEFAULT_OVERLAY_THEME,
                OverlayContentKind::Lyrics,
                1.0,
                "l1",
            ),
            create_custom_theme(
                "A",
                &DEFAULT_OVERLAY_THEME,
                OverlayContentKind::Scripture,
                1.0,
                "s1",
            ),
            create_custom_theme(
                "B",
                &DEFAULT_OVERLAY_THEME,
                OverlayContentKind::Scripture,
                1.0,
                "s2",
            ),
        ]
    }

    fn screen_with(theme: &CustomOverlayTheme) -> OverlayOutput {
        let mut o = make_overlay_output("screen", kairo_domain::overlay::OverlayOutputKind::Screen);
        o.enabled = true;
        assign_theme_to_output(&o, theme)
    }

    #[test]
    fn opens_the_remembered_theme_first() {
        let l = lib();
        let outputs = [screen_with(&l[2])];
        assert_eq!(
            initial_selection(&l, &outputs, Some("s1")).as_deref(),
            Some("s1")
        );
    }

    #[test]
    fn then_the_primary_screens_theme() {
        let l = lib();
        let outputs = [screen_with(&l[2])];
        assert_eq!(initial_selection(&l, &outputs, None).as_deref(), Some("s2"));
        // A remembered id that was deleted is ignored.
        assert_eq!(
            initial_selection(&l, &outputs, Some("gone")).as_deref(),
            Some("s2")
        );
    }

    #[test]
    fn then_the_first_scripture_theme_then_anything() {
        let l = lib();
        assert_eq!(initial_selection(&l, &[], None).as_deref(), Some("s1"));
        let only_lyrics = vec![l[0].clone()];
        assert_eq!(
            initial_selection(&only_lyrics, &[], None).as_deref(),
            Some("l1")
        );
        assert_eq!(initial_selection(&[], &[], None), None);
    }

    #[test]
    fn samples_follow_the_kind_and_length() {
        let s = sample(OverlayContentKind::Scripture, false);
        assert_eq!(s.reference, "John 3:16 (KJV)");
        assert!(sample(OverlayContentKind::Scripture, true).text.len() > s.text.len());
        let lyric = sample(OverlayContentKind::Lyrics, false);
        assert!(lyric.reference.starts_with("Amazing Grace"));
        assert!(lyric.text.contains('\n'));
    }

    #[test]
    fn the_frame_is_the_widest_16_by_9_that_fits() {
        // Wide and short: height limits.
        assert_eq!(
            fit_frame_width((1200.0, 500.0), (24.0, 64.0, 24.0, 24.0)),
            732.0
        );
        // Narrow and tall: width limits.
        assert_eq!(
            fit_frame_width((400.0, 900.0), (24.0, 64.0, 24.0, 24.0)),
            352.0
        );
        // No room at all.
        assert_eq!(fit_frame_width((10.0, 10.0), (24.0, 64.0, 24.0, 24.0)), 0.0);
    }

    #[test]
    fn media_takes_its_own_shape() {
        let wide = media_box(Some(16.0 / 9.0), OverlayElementKind::Image);
        assert!((wide.width_pct - 30.0).abs() < 1e-9);
        // 30 % of 1920 is 576 px; at 16:9 that is 324 px tall: 30 % of 1080.
        assert!((wide.height_pct - 30.0).abs() < 1e-9);
        let tall = media_box(Some(0.25), OverlayElementKind::Image);
        assert!((tall.height_pct - 70.0).abs() < 1e-9);
        assert!(tall.width_pct < 30.0);
        assert!((tall.x_pct + tall.width_pct / 2.0 - 50.0).abs() < 1e-9);
    }

    #[test]
    fn unmeasurable_media_gets_the_default_box() {
        let d = media_box(None, OverlayElementKind::Video);
        assert_eq!(
            d,
            create_overlay_element(OverlayElementKind::Video, None, "", None).frame
        );
        assert_eq!(media_box(Some(0.0), OverlayElementKind::Video), d);
        assert_eq!(media_box(Some(f64::NAN), OverlayElementKind::Video), d);
    }

    #[test]
    fn readout_is_in_output_pixels() {
        let r = frame_readout(&overlay_box(50.0, 25.0, 10.0, 20.0));
        assert_eq!(r, [("X", 960), ("Y", 270), ("W", 192), ("H", 216)]);
    }

    #[test]
    fn media_rows_show_the_file_name() {
        let mut el =
            create_overlay_element(OverlayElementKind::Image, Some("/a/b/logo.png"), "x", None);
        assert_eq!(element_name(&el), "logo.png");
        el.media_path = Some("C:\\media\\clip.mp4".into());
        assert_eq!(element_name(&el), "clip.mp4");
        el.media_path = None;
        assert_eq!(element_name(&el), "Image");
    }

    #[test]
    fn ids_do_not_repeat() {
        assert_ne!(new_id("theme", 1.0), new_id("theme", 1.0));
    }

    #[test]
    fn a_fresh_theme_is_its_own_default() {
        let item = create_custom_theme(
            "A",
            &DEFAULT_OVERLAY_THEME,
            OverlayContentKind::Scripture,
            1.0,
            "a",
        );
        assert!(is_default(&normalized(&item.theme), &item));
        let mut edited = normalized(&item.theme);
        edited.verse.font_size_px += 1.0;
        assert!(!is_default(&edited, &item));
    }
}
