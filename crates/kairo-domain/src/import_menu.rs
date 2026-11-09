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
