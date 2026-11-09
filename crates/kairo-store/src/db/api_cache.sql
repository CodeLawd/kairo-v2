CREATE TABLE IF NOT EXISTS api_translations (
  bible_id TEXT PRIMARY KEY,
  translation_id TEXT NOT NULL,
  name TEXT NOT NULL,
  copyright TEXT NOT NULL,
  status TEXT NOT NULL,
  total_chapters INTEGER NOT NULL DEFAULT 0,
  last_access_check INTEGER,
  last_error TEXT
);

CREATE TABLE IF NOT EXISTS api_verses (
  bible_id TEXT NOT NULL,
  book_id INTEGER NOT NULL,
  book_name TEXT NOT NULL,
  chapter INTEGER NOT NULL,
  verse INTEGER NOT NULL,
  ciphertext TEXT NOT NULL,
  iv TEXT NOT NULL,
  auth_tag TEXT NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (bible_id, book_id, chapter, verse),
  FOREIGN KEY (bible_id) REFERENCES api_translations(bible_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS api_ranges (
  bible_id TEXT NOT NULL,
  book_id INTEGER NOT NULL,
  chapter INTEGER NOT NULL,
  verse_start INTEGER NOT NULL,
  verse_end INTEGER NOT NULL,
  fetched_at INTEGER NOT NULL,
  PRIMARY KEY (bible_id, book_id, chapter, verse_start, verse_end),
  FOREIGN KEY (bible_id) REFERENCES api_translations(bible_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS api_chapters (
  bible_id TEXT NOT NULL,
  chapter_id TEXT NOT NULL,
  status TEXT NOT NULL,
  fetched_at INTEGER,
  PRIMARY KEY (bible_id, chapter_id),
  FOREIGN KEY (bible_id) REFERENCES api_translations(bible_id) ON DELETE CASCADE
);
