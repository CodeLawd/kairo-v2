//! The delete handlers' clean-up: a deleted song must not linger in a
//! setlist as a dead row, nor keep a library assignment pointing at
//! nothing; a removed document leaves its library too.

use crate::{LibrariesService, Result, SetlistService};
use kairo_domain::collections::LibraryKind;
use kairo_lyrics::LyricsService;

/// `lyrics:delete`. The clean-up runs whether or not the song existed, and
/// not when the delete itself failed, as in Electron.
pub async fn delete_song(
    lyrics: &LyricsService,
    setlist: &SetlistService,
    libraries: &LibrariesService,
    song_id: &str,
) -> Result<bool> {
    let result = lyrics.delete_song(song_id).await?;
    setlist.forget_song(song_id);
    libraries.forget_item(LibraryKind::Songs, song_id);
    Ok(result)
}

/// The library half of `documents:remove`. The documents service (045)
/// calls this before removing the document itself.
pub fn remove_document_from_libraries(libraries: &LibrariesService, document_id: &str) {
    libraries.forget_item(LibraryKind::Documents, document_id);
}
