//! The Import menu, from `src/lib/import-menu.ts`.

use crate::serde_util::string_enum;

string_enum!(
    pub enum ImportKind {
        Pdf = "pdf",
        PowerPoint = "powerpoint",
        Image = "image",
        Video = "video",
        Audio = "audio",
        Sermon = "sermon",
        Lyrics = "lyrics",
        Kairo = "kairo",
    }
);

pub const IMPORT_OPTIONS: &[(ImportKind, &str, Option<&str>)] = &[
    (ImportKind::Pdf, "PDF document…", Some("documents")),
    (
        ImportKind::PowerPoint,
        "PowerPoint presentation…",
        Some("documents"),
    ),
    (ImportKind::Image, "Images…", Some("operator")),
    (ImportKind::Video, "Videos…", Some("operator")),
    (ImportKind::Audio, "Audio files…", Some("operator")),
    (ImportKind::Sermon, "Sermon notes…", Some("scripture")),
    (ImportKind::Lyrics, "Lyrics file…", Some("lyrics")),
    (ImportKind::Kairo, "Kairo file (.kairo)…", None),
];

pub fn is_import_kind(value: &str) -> bool {
    ImportKind::from_name(value).is_some()
}
