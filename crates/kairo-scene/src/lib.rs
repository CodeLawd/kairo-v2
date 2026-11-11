//! Slide scenes and their CPU raster.
//!
//! [`SlideScene::build`] turns a theme, a reference, a verse and optional
//! coloured lines into typed boxes of text. [`rasterize`] draws them with
//! parley (shaping) and swash (outlines) into a premultiplied RGBA tile, and
//! [`RasterPool`] does that on worker threads with a cache keyed by the
//! scene's content hash. The engine and the UI process use the same code.

pub mod color;
pub mod fit;
mod glyphs;
pub mod media;
mod pool;
mod raster;
mod scene;
mod text;
mod typeset;
pub mod vector;

pub use pool::{Job, RasterPool};
pub use raster::{
    BoxLayout, TextRaster, Timings, analyse, rasterize, rasterize_timed, rasterize_with,
};
pub use scene::{
    Align, BoxKind, ColoredLine, Decoration, Fit, Paragraph, Rect, Shadow, SlideScene, Stroke,
    TextBox, TextStyle, Vertical, colored_lines_from_json,
};
pub use text::{collapse, paragraphs_for, transform};
pub use typeset::{
    FONT_STACKS, Typesetter, bundled_families, bundled_font_covers, family_chain, with_typesetter,
};
