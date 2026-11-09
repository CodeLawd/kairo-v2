CREATE TABLE IF NOT EXISTS translations (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    language   TEXT NOT NULL DEFAULT 'en',
    is_default INTEGER NOT NULL DEFAULT 0
  );

CREATE TABLE IF NOT EXISTS books (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL,
    abbreviation TEXT NOT NULL,
    testament    TEXT NOT NULL CHECK(testament IN ('OT', 'NT')),
    aliases      TEXT NOT NULL DEFAULT '[]'
  );

CREATE TABLE IF NOT EXISTS verses (
    translation_id TEXT    NOT NULL REFERENCES translations(id),
    book_id        INTEGER NOT NULL REFERENCES books(id),
    chapter        INTEGER NOT NULL,
    verse          INTEGER NOT NULL,
    text           TEXT    NOT NULL,
    PRIMARY KEY (translation_id, book_id, chapter, verse)
  );

CREATE INDEX IF NOT EXISTS idx_verses_lookup
    ON verses (translation_id, book_id, chapter, verse);

CREATE VIRTUAL TABLE IF NOT EXISTS verses_fts USING fts5(
    text,
    translation_id UNINDEXED,
    book_id        UNINDEXED,
    chapter        UNINDEXED,
    verse          UNINDEXED
  );
