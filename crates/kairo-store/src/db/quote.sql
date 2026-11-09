CREATE TABLE books (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE VIRTUAL TABLE verses_fts USING fts5(text, translation_id UNINDEXED,
  book_id UNINDEXED, chapter UNINDEXED, verse UNINDEXED);
CREATE TABLE quote_sources (row_id INTEGER PRIMARY KEY, bible_id TEXT NOT NULL,
  book_id INTEGER NOT NULL, book_name TEXT NOT NULL, chapter INTEGER NOT NULL,
  verse INTEGER NOT NULL, translation TEXT, fetched_at INTEGER NOT NULL, expires_at INTEGER NOT NULL);
CREATE INDEX quote_expiry ON quote_sources(expires_at);
CREATE INDEX quote_identity ON quote_sources(bible_id,book_name,chapter,verse);
