CREATE TABLE IF NOT EXISTS songs (
    id          TEXT    PRIMARY KEY,
    title       TEXT    NOT NULL COLLATE NOCASE,
    artist      TEXT    NOT NULL DEFAULT '' COLLATE NOCASE,
    copyright   TEXT    NOT NULL DEFAULT '',
    ccli_number TEXT,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    source      TEXT    NOT NULL DEFAULT 'text',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
  );

CREATE INDEX IF NOT EXISTS idx_songs_title  ON songs(title   COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_songs_artist ON songs(artist  COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS idx_songs_fav    ON songs(is_favorite);

CREATE UNIQUE INDEX IF NOT EXISTS idx_songs_ccli
    ON songs(ccli_number) WHERE ccli_number IS NOT NULL AND ccli_number != '';

CREATE TABLE IF NOT EXISTS song_sections (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    song_id     TEXT    NOT NULL REFERENCES songs(id) ON DELETE CASCADE,
    type        TEXT    NOT NULL,
    label       TEXT    NOT NULL,
    section_idx INTEGER NOT NULL DEFAULT 1,
    lines       TEXT    NOT NULL DEFAULT '[]',
    sort_order  INTEGER NOT NULL DEFAULT 0
  );

CREATE INDEX IF NOT EXISTS idx_sections_song ON song_sections(song_id);

CREATE VIRTUAL TABLE IF NOT EXISTS songs_fts USING fts5(
    title,
    artist,
    lyrics,
    song_id UNINDEXED
  );
