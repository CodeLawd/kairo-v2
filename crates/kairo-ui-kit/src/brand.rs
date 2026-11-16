//! The Kairo mark and lockup (`components/brand/KairoMark.tsx`).
//!
//! - [`KairoMark`] with [`MarkVariant::Mark`]: the bare mark in the theme's
//!   text colour. Used everywhere inside the app; blue is kept for what is
//!   live, so the brand stays neutral.
//! - [`MarkVariant::Tile`]: the off-white mark on the blue app-icon tile.
//! - [`KairoLockup`]: the wordmark, optionally with the tagline.
//!
//! Electron tints the PNGs with a CSS mask. Here the PNG's alpha is
//! recoloured once per colour and cached, which draws the same pixels.

use crate::theme::KairoThemeExt;
use crate::tokens::{TextStep, text};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::v_flex;
use gpui_kit::{
    App, Global, Hsla, ImageSource, InteractiveElement, IntoElement, ParentElement, Pixels,
    RenderImage, RenderOnce, Rgba, Role, SharedString, StatefulInteractiveElement, Styled, Window,
    div, img, px,
};
use image::{Frame, ImageFormat, RgbaImage};
use std::collections::HashMap;
use std::sync::Arc;

/// `PRODUCT_NAME`.
pub const PRODUCT_NAME: &str = "Kairo";
/// `PRODUCT_TAGLINE`.
pub const PRODUCT_TAGLINE: &str = "Scripture  |  Lyrics  |  Presentation";

const MARK_PNG: &[u8] = include_bytes!("../assets/brand/kairo-mark.png");
const TILE_PNG: &[u8] = include_bytes!("../assets/brand/kairo-tile.png");
const WORDMARK_PNG: &[u8] = include_bytes!("../assets/brand/kairo-wordmark.png");

/// The wordmark's aspect ratio (1002 / 240).
const WORDMARK_RATIO: f32 = 1002.0 / 240.0;

/// Mark sizes (`sm` 28, `md` 56, `lg` 72).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkSize {
    /// 28px.
    Small,
    /// 56px.
    #[default]
    Medium,
    /// 72px.
    Large,
}

impl MarkSize {
    fn px(self) -> f32 {
        match self {
            MarkSize::Small => 28.0,
            MarkSize::Medium => 56.0,
            MarkSize::Large => 72.0,
        }
    }

    fn lockup_height(self) -> f32 {
        match self {
            MarkSize::Small => 24.0,
            MarkSize::Medium => 32.0,
            MarkSize::Large => 40.0,
        }
    }
}

/// Which artwork.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MarkVariant {
    /// The bare mark in the text colour.
    #[default]
    Mark,
    /// The app-icon tile.
    Tile,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Art {
    Mark,
    Wordmark,
}

#[derive(Default)]
struct TintCache(HashMap<(Art, [u8; 4]), Arc<RenderImage>>);

impl Global for TintCache {}

fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    match image::load_from_memory_with_format(bytes, ImageFormat::Png) {
        Ok(image) => Some(image.into_rgba8()),
        Err(error) => {
            tracing::warn!(%error, "bundled brand image could not be decoded");
            None
        }
    }
}

/// The artwork's alpha filled with `color`, as GPUI's BGRA.
pub fn tint(bytes: &[u8], color: Hsla) -> Option<RgbaImage> {
    let mut image = decode(bytes)?;
    let rgba: Rgba = color.into();
    let [r, g, b] = [rgba.r, rgba.g, rgba.b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    for pixel in image.pixels_mut() {
        let alpha = (f32::from(pixel.0[3]) * rgba.a.clamp(0.0, 1.0)).round() as u8;
        pixel.0 = [b, g, r, alpha];
    }
    Some(image)
}

fn bgra(bytes: &[u8]) -> Option<RgbaImage> {
    let mut image = decode(bytes)?;
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(image)
}

fn tinted(art: Art, color: Hsla, cx: &mut App) -> Option<Arc<RenderImage>> {
    let rgba: Rgba = color.into();
    let key = [rgba.r, rgba.g, rgba.b, rgba.a].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    if let Some(found) = cx.default_global::<TintCache>().0.get(&(art, key)) {
        return Some(found.clone());
    }
    let bytes = match art {
        Art::Mark => MARK_PNG,
        Art::Wordmark => WORDMARK_PNG,
    };
    let image = Arc::new(RenderImage::new([Frame::new(tint(bytes, color)?)]));
    cx.default_global::<TintCache>()
        .0
        .insert((art, key), image.clone());
    Some(image)
}

struct TileImage(Option<Arc<RenderImage>>);
impl Global for TileImage {}

fn tile(cx: &mut App) -> Option<Arc<RenderImage>> {
    if !cx.has_global::<TileImage>() {
        let image = bgra(TILE_PNG).map(|image| Arc::new(RenderImage::new([Frame::new(image)])));
        cx.set_global(TileImage(image));
    }
    cx.global::<TileImage>().0.clone()
}

/// The Kairo mark. Decorative: hidden from assistive technology, as the
/// original's `aria-hidden`.
#[derive(IntoElement, Default)]
pub struct KairoMark {
    size: MarkSize,
    variant: MarkVariant,
}

impl KairoMark {
    /// A mark of `size`.
    pub fn new(size: MarkSize) -> Self {
        Self {
            size,
            variant: MarkVariant::Mark,
        }
    }

    /// The app-icon tile instead of the bare mark.
    pub fn tile(mut self) -> Self {
        self.variant = MarkVariant::Tile;
        self
    }
}

fn picture(image: Option<Arc<RenderImage>>, width: Pixels, height: Pixels) -> gpui_kit::AnyElement {
    match image {
        Some(image) => img(ImageSource::Render(image))
            .w(width)
            .h(height)
            .flex_shrink_0()
            .into_any_element(),
        // A broken bundled file leaves the space empty rather than failing.
        None => div().w(width).h(height).flex_shrink_0().into_any_element(),
    }
}

impl RenderOnce for KairoMark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let side = self.size.px();
        match self.variant {
            MarkVariant::Mark => {
                // The mask box is 72% of the size.
                let box_side = px((side * 0.72).round());
                let color = cx.kairo().text_primary;
                picture(tinted(Art::Mark, color, cx), box_side, box_side)
            }
            MarkVariant::Tile => picture(tile(cx), px(side), px(side)),
        }
    }
}

/// The wordmark, as drawn by the brand, with an optional tagline.
#[derive(IntoElement, Default)]
pub struct KairoLockup {
    size: MarkSize,
    tagline: bool,
    start: bool,
}

impl KairoLockup {
    /// A lockup of `size`, centred.
    pub fn new(size: MarkSize) -> Self {
        Self {
            size,
            tagline: false,
            start: false,
        }
    }

    /// Shows the tagline under the wordmark.
    pub fn tagline(mut self, tagline: bool) -> Self {
        self.tagline = tagline;
        self
    }

    /// Aligns to the start instead of the centre.
    pub fn align_start(mut self) -> Self {
        self.start = true;
        self
    }
}

impl RenderOnce for KairoLockup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let height = self.size.lockup_height();
        let image = tinted(Art::Wordmark, k.text_primary, cx);
        let column = v_flex().gap_3();
        let column = if self.start {
            column.items_start()
        } else {
            column.items_center()
        };
        column
            .child(
                div()
                    .id("kairo-lockup")
                    .test_support()
                    .role(Role::Image)
                    .aria_label(SharedString::new_static(PRODUCT_NAME))
                    .child(picture(image, px(height * WORDMARK_RATIO), px(height))),
            )
            .when_tagline(self.tagline, k.text(TextStep::T400))
    }
}

trait TaglineExt: ParentElement + Sized {
    fn when_tagline(self, show: bool, color: Hsla) -> Self {
        if !show {
            return self;
        }
        self.child(
            div()
                .text_size(text::MICRO.size)
                .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(PRODUCT_TAGLINE.to_uppercase()),
        )
    }
}

impl<E: ParentElement> TaglineExt for E {}

const GOOGLE_SVG: &[u8] = include_bytes!("../assets/brand/google.svg");

struct GoogleLogo(Arc<gpui_kit::Image>);
impl Global for GoogleLogo {}

/// The Google "G" in a transparent 18 px slot. Decorative; the button names it.
pub fn google_logo(cx: &mut App) -> impl IntoElement {
    if !cx.has_global::<GoogleLogo>() {
        let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Svg, GOOGLE_SVG.to_vec());
        cx.set_global(GoogleLogo(Arc::new(image)));
    }
    let image = cx.global::<GoogleLogo>().0.clone();
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(18.))
        .overflow_hidden()
        .child(img(image).size(px(18.)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_artwork_decodes_at_its_known_sizes() {
        let mark = decode(MARK_PNG).unwrap();
        assert_eq!((mark.width(), mark.height()), (504, 512));
        let word = decode(WORDMARK_PNG).unwrap();
        assert_eq!((word.width(), word.height()), (1002, 240));
        assert_eq!(decode(TILE_PNG).unwrap().width(), 512);
    }

    #[test]
    fn tint_keeps_the_shape_and_swaps_to_bgra() {
        let red: Hsla = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
        .into();
        let tinted = tint(MARK_PNG, red).unwrap();
        let original = decode(MARK_PNG).unwrap();
        for (a, b) in tinted.pixels().zip(original.pixels()) {
            assert_eq!(a.0[3], b.0[3]);
            assert_eq!(&a.0[..3], &[0, 0, 255]);
        }
    }

    #[test]
    fn a_broken_file_is_none_not_a_panic() {
        assert!(decode(b"not a png").is_none());
    }
}
