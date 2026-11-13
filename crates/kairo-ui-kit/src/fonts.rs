//! Source Sans 3, embedded and registered through GPUI's font API.
//!
//! The variable subsets of `@fontsource-variable/source-sans-3` 5.3.0
//! (`crates/kairo-scene/fonts`, SIL OFL 1.1) register on macOS as "Source
//! Sans 3 ExtraLight", their default instance, so the interface could not
//! ask for them by family. `tools/instance-fonts.py` writes static Regular,
//! Medium, Semibold and Bold faces with the five script subsets merged
//! (Yoruba needs Latin Extended and Vietnamese), named "Source Sans 3".

use gpui_kit::App;
use std::borrow::Cow;

/// The bundled weights: 400, 500, 600 and 700.
pub const FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/SourceSans3-Regular.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Medium.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Semibold.ttf"),
    include_bytes!("../assets/fonts/SourceSans3-Bold.ttf"),
];

/// Registers Source Sans 3 with the text system. A failure is logged and
/// the system font stays in use, as a browser would fall back.
pub fn register(cx: &mut App) {
    let fonts = FILES.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(%error, "Source Sans 3 could not be registered; using the system font");
    }
}
