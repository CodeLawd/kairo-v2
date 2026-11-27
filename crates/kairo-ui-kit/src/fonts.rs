//! The interface font: Inter 4.1 (rsms/inter, SIL OFL 1.1,
//! `assets/fonts/Inter-OFL.txt`), embedded and registered through GPUI's
//! font API. The static Regular, Medium, SemiBold and Bold faces from the
//! release's `extras/ttf` register as family "Inter", so the interface asks
//! for it by name (the variable font would register under its default
//! instance). They cover Latin Extended and Vietnamese, which Yoruba needs.
//!
//! The Source Sans 3 faces beside them are what the interface used before;
//! screens and feeds still draw with Source Sans (`kairo-scene`).

use gpui_kit::App;
use std::borrow::Cow;

/// The bundled weights: 400, 500, 600 and 700.
pub const FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
];

/// Registers Inter with the text system. A failure is logged and the system
/// font stays in use, as a browser would fall back.
pub fn register(cx: &mut App) {
    let fonts = FILES.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        tracing::warn!(%error, "Inter could not be registered; using the system font");
    }
}
