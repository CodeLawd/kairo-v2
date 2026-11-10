//! Bible lookup, search and local packs, from `scripture/index.ts`.
//!
//! The quote worker (prompt 037), API.Bible (034) and the orchestrator's
//! suggestion flow (039) attach to this service; it owns the local Bible.

use crate::api::{ApiBible, ApiChapter, ApiError, ApiPassage};
use crate::bible_db::{BibleDb, BibleVerse};
use crate::installer::{
    InstallError, InstallFailure, InstallerBackend, LocalBibleInstaller, Unsubscribe,
};
use crate::pack::temporary_pack_path;
use crate::pack::{PackFetcher, TransferProgress, download_bible_pack, pack_status_for};
use crate::reference::{
    ParsedScriptureReference, parse_scripture_reference, resolve_reference_lookup,
};
use kairo_domain::js::{is_whitespace, math_max, math_min, number_to_string, trim};
use kairo_domain::scripture::translations::{
    DEFAULT_TRANSLATION_ID, get_downloadable_pack, get_translation_definition,
    is_bundled_translation, translation_catalog_entries,
};
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBibleInstallProgress, LocalBiblePackInstallResult,
    LocalBiblePackStatus, ScriptureResult, ScriptureRowTranslation, ScriptureSuggestion,
    ScriptureTranslationOption, ScriptureVerse, TranslationAccess,
};
use kairo_domain::serde_util::True;
use kairo_store::db::validate_bible_pack;
use regex::Regex;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Display;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};

/// Errors whose messages are safe to show the operator, as Electron's are.
#[derive(Debug, thiserror::Error)]
pub enum ScriptureError {
    #[error("Scripture database is not open.")]
    NotOpen,
    #[error(transparent)]
    Database(#[from] kairo_store::db::Error),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("The bundled {0} copy cannot be overwritten by a pack.")]
    BundledOverwrite(String),
    #[error("The bundled {0} copy cannot be removed.")]
    BundledRemoval(String),
    #[error("No downloadable local pack is available for {0}.")]
    NotDownloadable(String),
    #[error("Wait for this Bible to finish installing before removing it.")]
    InstallInProgress,
    #[error(transparent)]
    Install(#[from] InstallError),
}

type Result<T> = std::result::Result<T, ScriptureError>;

/// A verse offered to quote recovery (`QuoteCandidate`).
#[derive(Clone, Debug, PartialEq)]
pub struct QuoteCandidate {
    pub book: String,
    pub chapter: f64,
    pub verse: f64,
    pub text: String,
}

type Callback<T> = Arc<dyn Fn(&T) + Send + Sync>;
type Entries<T> = Arc<Mutex<Vec<(u64, Callback<T>)>>>;

/// Registered callbacks of one kind, called in registration order.
struct Listeners<T> {
    entries: Entries<T>,
    next: Mutex<u64>,
}

impl<T: 'static> Listeners<T> {
    fn new() -> Self {
        Self {
            entries: Arc::default(),
            next: Mutex::new(0),
        }
    }

    fn add(&self, callback: Callback<T>) -> Subscription {
        let id = {
            let mut next = lock(&self.next);
            *next += 1;
            *next
        };
        lock(&self.entries).push((id, callback));
        let entries = Arc::downgrade(&self.entries);
        Subscription(Box::new(move || {
            if let Some(entries) = entries.upgrade() {
                lock(&entries).retain(|(entry, _)| *entry != id);
            }
        }))
    }

    /// Calls each callback outside the lock, so callbacks may subscribe.
    fn emit(&self, value: &T) {
        let callbacks: Vec<_> = lock(&self.entries).iter().map(|(_, c)| c.clone()).collect();
        for callback in callbacks {
            callback(value);
        }
    }
}

/// Ends a subscription when [`Subscription::unsubscribe`] is called.
pub struct Subscription(Box<dyn FnOnce() + Send>);

impl Subscription {
    pub fn unsubscribe(self) {
        (self.0)()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct State {
    db: Option<BibleDb>,
    api: Option<Arc<dyn ApiBible>>,
    /// Catalogue ids the attached API.Bible source last authorized.
    api_translation_ids: BTreeSet<String>,
    default_translation: String,
    auto_mode: bool,
    confidence_threshold: f64,
    pending_suggestions: HashMap<String, ScriptureSuggestion>,
}

struct Inner {
    state: Mutex<State>,
    installer: LocalBibleInstaller,
    suggestions: Listeners<ScriptureSuggestion>,
    row_translations: Listeners<ScriptureRowTranslation>,
    corpus: Listeners<()>,
}

/// The scripture service. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct ScriptureService(Arc<Inner>);

/// Downloads with the given fetcher and installs into the owning service.
struct PackInstall {
    service: Weak<Inner>,
    fetcher: Arc<dyn PackFetcher>,
}

impl InstallerBackend for PackInstall {
    fn download<'a>(
        &'a self,
        translation: &'a str,
        update: &'a mut (dyn FnMut(TransferProgress) + Send),
    ) -> crate::api::BoxFuture<'a, std::result::Result<Vec<u8>, InstallFailure>> {
        Box::pin(async move {
            Ok(download_bible_pack(translation, &*self.fetcher, None, update).await?)
        })
    }

    fn install<'a>(
        &'a self,
        translation: &'a str,
        sqlite: Vec<u8>,
    ) -> crate::api::BoxFuture<'a, std::result::Result<LocalBiblePackInstallResult, InstallFailure>>
    {
        Box::pin(async move {
            let other = |error: &dyn Display| InstallFailure::Other(error.to_string());
            let service = self
                .service
                .upgrade()
                .map(ScriptureService)
                .ok_or_else(|| other(&"The scripture service has closed."))?;
            let path = temporary_pack_path(Some(translation));
            write_private_new(&path, &sqlite).map_err(|e| other(&e))?;
            let result = service
                .install_local_bible_pack(&path, Some(translation))
                .await;
            // Best-effort temporary cleanup.
            let _ = std::fs::remove_file(&path);
            result.map_err(|e| other(&e))
        })
    }
}

/// `writeFileSync(path, bytes, { flag: 'wx', mode: 0o600 })`.
fn write_private_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

static API_REFERENCE_BOOK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([^\n\r\u{2028}\u{2029}]+?)[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]+[0-9]+:[0-9]+").unwrap()
});
static QUOTE_FILLER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:\b(?:the|has|hath|been|of|you|me|us|and|is|he|it)\b)").unwrap()
});

/// JavaScript truthiness for a number.
fn truthy(n: Option<f64>) -> bool {
    n.is_some_and(|n| n != 0.0 && !n.is_nan())
}

fn to_verse(book: String, chapter: f64, verse: f64, text: String) -> ScriptureVerse {
    ScriptureVerse {
        book,
        chapter,
        verse,
        text,
        extra: Default::default(),
    }
}

/// Rows as single-verse results; an empty row translation takes `fallback`.
fn map_rows(rows: Vec<BibleVerse>, fallback: &str) -> Vec<ScriptureResult> {
    rows.into_iter()
        .map(|v| {
            let translation = if v.translation_id.is_empty() {
                fallback.to_owned()
            } else {
                v.translation_id
            };
            ScriptureResult {
                reference: format!("{} {}:{}", v.book_name, v.chapter, v.verse),
                verses: vec![to_verse(
                    v.book_name,
                    v.chapter as f64,
                    v.verse as f64,
                    v.text,
                )],
                translation,
            }
        })
        .collect()
}

impl ScriptureService {
    /// A closed service. `fetcher` downloads registry packs; production
    /// passes [`crate::pack::HttpPackFetcher`].
    pub fn new(fetcher: Arc<dyn PackFetcher>) -> Self {
        Self(Arc::new_cyclic(|service| Inner {
            state: Mutex::new(State {
                db: None,
                api: None,
                api_translation_ids: BTreeSet::new(),
                default_translation: DEFAULT_TRANSLATION_ID.to_owned(),
                auto_mode: false,
                confidence_threshold: 0.7,
                pending_suggestions: HashMap::new(),
            }),
            installer: LocalBibleInstaller::new(Arc::new(PackInstall {
                service: service.clone(),
                fetcher,
            })),
            suggestions: Listeners::new(),
            row_translations: Listeners::new(),
            corpus: Listeners::new(),
        }))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.0.state)
    }

    fn db(&self) -> Option<BibleDb> {
        self.state().db.clone()
    }

    fn require_db(&self) -> Result<BibleDb> {
        self.db().ok_or(ScriptureError::NotOpen)
    }

    // ─── Lifecycle ────────────────────────────────────────────────────────────

    /// Open the writable profile copy, `user_data/bible.db`. It is copied from
    /// the bundle on first run only, so an installed pack survives upgrades.
    pub async fn open(&self, user_data: &Path, bundled_bible: &Path) -> Result<()> {
        let db = BibleDb::open_profile(user_data, bundled_bible).await?;
        self.attach_db(db);
        Ok(())
    }

    /// Use an already open database (tests, tools).
    pub fn attach_db(&self, db: BibleDb) {
        self.state().db = Some(db);
        self.notify_corpus_change();
    }

    pub fn is_open(&self) -> bool {
        self.state().db.is_some()
    }

    /// Attach API.Bible, or detach it with `None`. Authorized ids never
    /// outlive the source that granted them.
    pub fn attach_api_bible(&self, api: Option<Arc<dyn ApiBible>>) {
        {
            let mut state = self.state();
            tracing::info!(attached = api.is_some(), "[Scripture] API.Bible cache");
            state.api = api;
            state.api_translation_ids.clear();
        }
        self.notify_corpus_change();
    }

    /// `initOfflineBibles`: open the encrypted API.Bible cache and attach it.
    /// Fails soft: without OS encryption the app still works, uncached.
    pub fn init_offline_bibles<E: Display>(
        &self,
        open: impl FnOnce() -> std::result::Result<Arc<dyn ApiBible>, E>,
    ) -> bool {
        match open() {
            Ok(api) => {
                self.attach_api_bible(Some(api));
                tracing::info!("[Scripture] Offline Bible cache ready");
                true
            }
            Err(error) => {
                self.attach_api_bible(None);
                tracing::warn!(%error, "[Scripture] Offline Bible cache disabled");
                false
            }
        }
    }

    /// Called when local or cached text changes, so quote matching rebuilds.
    pub fn on_corpus_change(&self, callback: impl Fn() + Send + Sync + 'static) -> Subscription {
        self.0.corpus.add(Arc::new(move |_: &()| callback()))
    }

    /// Listener failures never interrupt a pack or cache change.
    fn notify_corpus_change(&self) {
        let _ = catch_unwind(AssertUnwindSafe(|| self.0.corpus.emit(&())));
    }

    // ─── Local Bible packs ────────────────────────────────────────────────────

    /// Whether `translation` is served from the local `bible.db` now.
    pub async fn is_local_translation(&self, translation: &str) -> Result<bool> {
        let Some(db) = self.db() else {
            return Ok(false);
        };
        let upper = translation.to_uppercase();
        Ok(db
            .get_all_translations()
            .await?
            .iter()
            .any(|item| item.id.to_uppercase() == upper))
    }

    /// Every non-bundled local translation, downloaded or imported, so each
    /// can be removed from Settings.
    pub async fn list_installed_local_bible_packs(&self) -> Result<Vec<InstalledLocalBiblePack>> {
        let db = self.require_db()?;
        let mut packs = Vec::new();
        for row in db.get_all_translations().await? {
            if is_bundled_translation(&row.id) {
                continue;
            }
            let status = self.get_local_bible_pack_status(&row.id).await?;
            let name = if !row.name.is_empty() {
                row.name
            } else if let Some(definition) = get_translation_definition(&row.id) {
                definition.name.to_owned()
            } else {
                row.id.to_uppercase()
            };
            packs.push(InstalledLocalBiblePack { name, status });
        }
        Ok(packs)
    }

    pub async fn get_local_bible_pack_status(
        &self,
        translation: &str,
    ) -> Result<LocalBiblePackStatus> {
        let db = self.require_db()?;
        let normalized = translation.to_uppercase();
        let has = db.has_translation(&normalized).await?;
        let (verses, chapters) = if has {
            (
                db.get_verse_count(&normalized).await?,
                db.get_chapter_count(&normalized).await?,
            )
        } else {
            (0, 0)
        };
        let mut status = pack_status_for(&normalized, verses, chapters, has);
        status.installation = self.0.installer.get_progress(&normalized);
        Ok(status)
    }

    /// Validate a single-translation pack file and replace that translation's
    /// rows in one transaction. Nothing is written when validation fails, and
    /// a failed import keeps the previous copy. With `expected_translation`
    /// (registry downloads always pass it) the pack must hold exactly that
    /// id. Bundled translations are refused: the profile copy is seeded once,
    /// so an overwrite could never be undone.
    pub async fn install_local_bible_pack(
        &self,
        pack_path: &Path,
        expected_translation: Option<&str>,
    ) -> Result<LocalBiblePackInstallResult> {
        let db = self.require_db()?;
        let pack = validate_bible_pack(pack_path, expected_translation).await?;
        if is_bundled_translation(&pack.translation_id) {
            return Err(ScriptureError::BundledOverwrite(pack.translation_id));
        }
        self.notify_corpus_change();
        let verse_count = pack.verses.len();
        db.import_translation_pack(
            &pack.translation_id,
            &pack.translation_name,
            &pack.language,
            pack.verses,
        )
        .await?;
        tracing::info!(translation = %pack.translation_id, verses = verse_count, "[Scripture] Local Bible pack installed");
        Ok(LocalBiblePackInstallResult {
            translation: pack.translation_id,
            verse_count: verse_count as f64,
            chapter_count: pack.chapter_count as f64,
            installed: True,
        })
    }

    /// Download, verify and install a registry pack (NKJV, NLT, AMPC).
    /// Duplicate requests share one download.
    pub async fn download_local_bible_translation(
        &self,
        translation: &str,
    ) -> Result<LocalBiblePackInstallResult> {
        let normalized = translation.to_uppercase();
        if get_downloadable_pack(&normalized).is_none() {
            return Err(ScriptureError::NotDownloadable(normalized));
        }
        Ok(self.0.installer.start(&normalized).await?)
    }

    pub fn on_local_bible_install_progress(
        &self,
        callback: impl Fn(&LocalBibleInstallProgress) + Send + Sync + 'static,
    ) -> Unsubscribe {
        self.0.installer.subscribe(callback)
    }

    /// Remove one locally installed translation and its FTS rows. Bundled
    /// translations can never be removed, whatever id is passed.
    pub async fn remove_local_bible_translation(
        &self,
        translation: &str,
    ) -> Result<LocalBiblePackStatus> {
        use kairo_domain::scripture::LocalBibleInstallPhase::{Failed, Ready};
        let db = self.require_db()?;
        let normalized = translation.to_uppercase();
        let phase = self.0.installer.get_progress(&normalized).map(|p| p.phase);
        if phase.is_some_and(|phase| phase != Ready && phase != Failed) {
            return Err(ScriptureError::InstallInProgress);
        }
        if is_bundled_translation(&normalized) {
            return Err(ScriptureError::BundledRemoval(normalized));
        }
        if db.has_translation(&normalized).await? {
            self.notify_corpus_change();
            db.remove_translation(&normalized).await?;
            tracing::info!(translation = %normalized, "[Scripture] Local Bible translation removed");
        }
        self.0.installer.clear(&normalized);
        self.get_local_bible_pack_status(&normalized).await
    }

    // ─── Suggestions and rows ────────────────────────────────────────────────

    pub fn on_suggestion(
        &self,
        callback: impl Fn(&ScriptureSuggestion) + Send + Sync + 'static,
    ) -> Subscription {
        self.0.suggestions.add(Arc::new(callback))
    }

    /// `register`: a suggestion from the orchestrator or the operator.
    pub fn receive_suggestion(&self, suggestion: ScriptureSuggestion) {
        {
            let mut state = self.state();
            if suggestion.retracted == Some(true) {
                state.pending_suggestions.remove(&suggestion.id);
            } else {
                state
                    .pending_suggestions
                    .insert(suggestion.id.clone(), suggestion.clone());
            }
        }
        self.0.suggestions.emit(&suggestion);
    }

    pub fn get_pending_suggestion(&self, id: &str) -> Option<ScriptureSuggestion> {
        self.state().pending_suggestions.get(id).cloned()
    }

    pub fn dismiss_suggestion(&self, id: &str) {
        self.state().pending_suggestions.remove(id);
        tracing::info!(suggestion_id = id, "Scripture suggestion dismissed");
    }

    /// The operator view owns a row's verses, so it switches; this only asks.
    pub fn on_row_translation(
        &self,
        callback: impl Fn(&ScriptureRowTranslation) + Send + Sync + 'static,
    ) -> Subscription {
        self.0.row_translations.add(Arc::new(callback))
    }

    pub fn request_row_translation(&self, passage_id: &str, translation: &str) {
        self.0.row_translations.emit(&ScriptureRowTranslation {
            passage_id: passage_id.to_owned(),
            translation: translation.to_owned(),
        });
    }

    // ─── Configuration ───────────────────────────────────────────────────────

    pub fn set_default_translation(&self, translation: &str) {
        self.state().default_translation = translation.to_owned();
        tracing::info!(translation, "Default translation updated");
    }

    pub fn default_translation(&self) -> String {
        self.state().default_translation.clone()
    }

    pub fn set_auto_mode(&self, enabled: bool) {
        self.state().auto_mode = enabled;
        tracing::info!(enabled, "Scripture auto mode");
    }

    pub fn auto_mode(&self) -> bool {
        self.state().auto_mode
    }

    /// `Math.max(0, Math.min(1, threshold))`.
    pub fn set_confidence_threshold(&self, threshold: f64) {
        let threshold = math_max(0.0, math_min(1.0, threshold));
        self.state().confidence_threshold = threshold;
        tracing::info!(threshold, "Confidence threshold updated");
    }

    pub fn confidence_threshold(&self) -> f64 {
        self.state().confidence_threshold
    }

    // ─── Quote recovery helpers ──────────────────────────────────────────────

    /// Local-only candidates for a damaged spoken citation.
    pub async fn search_local_quote_candidates(&self, phrase: &str) -> Result<Vec<QuoteCandidate>> {
        let Some(db) = self.db() else {
            return Ok(Vec::new());
        };
        let query = QUOTE_FILLER.replace_all(phrase, " ");
        Ok(db
            .search_text(&query, None, 50)
            .await?
            .into_iter()
            .map(|v| QuoteCandidate {
                book: v.book_name,
                chapter: v.chapter as f64,
                verse: v.verse as f64,
                text: v.text,
            })
            .collect())
    }

    /// One chapter in every installed translation, for chapter-scoped matching.
    pub async fn get_local_chapter_verses(
        &self,
        book: &str,
        chapter: f64,
    ) -> Result<Vec<QuoteCandidate>> {
        let Some(db) = self.db() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for translation in db.get_all_translations().await? {
            for v in db.get_chapter(&translation.id, book, chapter).await? {
                out.push(QuoteCandidate {
                    book: book.to_owned(),
                    chapter: v.chapter as f64,
                    verse: v.verse as f64,
                    text: v.text,
                });
            }
        }
        Ok(out)
    }

    // ─── Lookup and search ───────────────────────────────────────────────────

    pub fn parse_reference(&self, input: &str) -> Option<ParsedScriptureReference> {
        parse_scripture_reference(input)
    }

    /// `scripture:search`. A reference resolves in the requested translation
    /// only: locally when installed, otherwise through API.Bible. A phrase
    /// prefers API.Bible unless the translation is an installed offline pack,
    /// then searches every local Bible, the requested one first.
    pub async fn search(
        &self,
        query: &str,
        translation: Option<&str>,
    ) -> Result<Vec<ScriptureResult>> {
        let (db, api, translation) = {
            let state = self.state();
            let translation =
                translation.map_or_else(|| state.default_translation.clone(), str::to_owned);
            (state.db.clone(), state.api.clone(), translation)
        };
        tracing::info!(query, translation = %translation, "Scripture search");
        let Some(db) = db else {
            tracing::warn!("Scripture DB not open");
            return Ok(Vec::new());
        };
        let local_ids: Vec<String> = db
            .get_all_translations()
            .await?
            .into_iter()
            .map(|item| item.id.to_uppercase())
            .collect();
        let upper = translation.to_uppercase();
        let preferred_is_local = local_ids.contains(&upper);

        if let Some(reference) = parse_scripture_reference(query) {
            return self
                .search_reference(&db, api, &reference, &translation, &local_ids)
                .await;
        }

        // An installed offline pack (NKJV, for example) is always served from
        // SQLite: it was installed so no key or network is needed. Every other
        // translation, bundled ones included, asks API.Bible first.
        let offline_pack_preferred =
            get_downloadable_pack(&translation).is_some() && preferred_is_local;
        if let Some(api) = api.filter(|_| !offline_pack_preferred) {
            match self
                .search_phrase_from_api(&db, &*api, query, &translation)
                .await
            {
                Ok(hits) if !hits.is_empty() => {
                    tracing::info!(query, translation = %translation, hits = hits.len(), "Scripture phrase search via API.Bible");
                    return Ok(hits);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(query, translation = %translation, %error, "[Scripture] API.Bible phrase search failed — trying local");
                }
            }
        }

        let mut rows = if preferred_is_local {
            db.search_text(query, Some(&translation), 50).await?
        } else {
            // An API-only translation searches every local Bible.
            db.search_text(query, None, 50).await?
        };
        if rows.is_empty() && preferred_is_local {
            rows = db.search_text(query, None, 50).await?;
        }
        if rows.is_empty() {
            for id in &local_ids {
                if preferred_is_local && *id == upper {
                    continue;
                }
                rows = db.search_text(query, Some(id), 50).await?;
                if !rows.is_empty() {
                    break;
                }
            }
        }
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        if preferred_is_local {
            let (mut preferred, others): (Vec<_>, Vec<_>) = rows
                .into_iter()
                .partition(|row| row.translation_id.to_uppercase() == upper);
            preferred.extend(others);
            rows = preferred;
        }
        tracing::info!(query, preferred = %translation, preferred_is_local, hits = rows.len(), "Scripture phrase search hits");
        let fallback = rows
            .first()
            .map(|row| row.translation_id.clone())
            .filter(|id| !id.is_empty())
            .unwrap_or(translation);
        rows.truncate(25);
        Ok(map_rows(rows, &fallback))
    }

    async fn search_reference(
        &self,
        db: &BibleDb,
        api: Option<Arc<dyn ApiBible>>,
        reference: &ParsedScriptureReference,
        translation: &str,
        local_ids: &[String],
    ) -> Result<Vec<ScriptureResult>> {
        let r = reference;
        resolve_reference_lookup(
            translation,
            local_ids,
            |tx: String| async move {
                if r.is_chapter {
                    self.lookup_chapter(&r.book, r.chapter, Some(&tx)).await
                } else {
                    let start = r.verse_start.unwrap_or(f64::NAN);
                    self.lookup_verses(&r.book, r.chapter, start, r.verse_end, Some(&tx))
                        .await
                }
            },
            api.map(|api| {
                move || async move {
                    if r.is_chapter {
                        self.lookup_chapter_from_api(db, &*api, r, translation)
                            .await
                    } else {
                        self.lookup_from_api(db, &*api, r, translation).await
                    }
                }
            }),
            |verses: Vec<ScriptureVerse>, tx: String| async move {
                let book_name = db
                    .resolve_book_name(&r.book)
                    .await?
                    .map_or_else(|| r.book.clone(), |book| book.name);
                let chapter = number_to_string(r.chapter);
                let reference = if r.is_chapter {
                    format!("{book_name} {chapter}")
                } else {
                    let start = number_to_string(r.verse_start.unwrap_or(f64::NAN));
                    let end = if truthy(r.verse_end) {
                        format!("–{}", number_to_string(r.verse_end.unwrap_or_default()))
                    } else {
                        String::new()
                    };
                    format!("{book_name} {chapter}:{start}{end}")
                };
                Ok(ScriptureResult {
                    reference,
                    verses,
                    translation: tx,
                })
            },
        )
        .await
    }

    async fn search_phrase_from_api(
        &self,
        db: &BibleDb,
        api: &dyn ApiBible,
        query: &str,
        translation: &str,
    ) -> Result<Vec<ScriptureResult>> {
        let bible_id = api.resolve_bible_id(translation).await?;
        let hits = api.search(&bible_id, query, 8).await?;
        let mut results = Vec::new();
        for hit in hits {
            let mut book = API_REFERENCE_BOOK
                .captures(&hit.reference)
                .map(|caps| trim(&caps[1]).to_owned())
                .unwrap_or_default();
            if book.is_empty()
                && let Some(id) = hit.book_id.as_deref().filter(|id| !id.is_empty())
            {
                book = match db.resolve_book_name(id).await? {
                    Some(resolved) => resolved.name,
                    None => id.to_owned(),
                };
            }
            if book.is_empty() {
                book = "Unknown".into();
            }
            let (Some(chapter), Some(verse)) = (hit.chapter, hit.verse) else {
                continue;
            };
            if !truthy(Some(chapter)) || !truthy(Some(verse)) || hit.text.is_empty() {
                continue;
            }
            let reference = if hit.reference.contains(':') {
                // `replace(/\s+/g, ' ').trim()`.
                hit.reference
                    .split(is_whitespace)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                format!(
                    "{book} {}:{}",
                    number_to_string(chapter),
                    number_to_string(verse)
                )
            };
            results.push(ScriptureResult {
                reference,
                translation: translation.to_owned(),
                verses: vec![to_verse(book, chapter, verse, hit.text)],
            });
        }
        Ok(results)
    }

    async fn lookup_from_api(
        &self,
        db: &BibleDb,
        api: &dyn ApiBible,
        r: &ParsedScriptureReference,
        translation: &str,
    ) -> Result<Vec<ScriptureResult>> {
        let bible_id = api.resolve_bible_id(translation).await?;
        let Some(book) = db.resolve_book_name(&r.book).await? else {
            return Ok(Vec::new());
        };
        Ok(api
            .lookup_passage(ApiPassage {
                bible_id,
                translation: translation.to_owned(),
                book_id: book.id,
                book_name: book.name,
                chapter: r.chapter,
                verse_start: r.verse_start.unwrap_or(1.0),
                verse_end: r.verse_end,
            })
            .await?)
    }

    async fn lookup_chapter_from_api(
        &self,
        db: &BibleDb,
        api: &dyn ApiBible,
        r: &ParsedScriptureReference,
        translation: &str,
    ) -> Result<Vec<ScriptureResult>> {
        let bible_id = api.resolve_bible_id(translation).await?;
        let Some(book) = db.resolve_book_name(&r.book).await? else {
            return Ok(Vec::new());
        };
        Ok(api
            .lookup_chapter(ApiChapter {
                bible_id,
                translation: translation.to_owned(),
                book_id: book.id,
                book_name: book.name,
                chapter: r.chapter,
            })
            .await?)
    }

    /// A whole chapter from the local Bible. `translation` defaults to the
    /// default translation.
    pub async fn lookup_chapter(
        &self,
        book: &str,
        chapter: f64,
        translation: Option<&str>,
    ) -> Result<Vec<ScriptureVerse>> {
        tracing::info!(book, chapter, "Scripture chapter lookup");
        let Some(db) = self.db() else {
            return Ok(Vec::new());
        };
        let translation = translation.map_or_else(|| self.default_translation(), str::to_owned);
        Ok(db
            .get_chapter(&translation, book, chapter)
            .await?
            .into_iter()
            .map(|v| to_verse(v.book_name, chapter, v.verse as f64, v.text))
            .collect())
    }

    /// One verse, or a range when `verse_end` is set (and not 0).
    pub async fn lookup_verses(
        &self,
        book: &str,
        chapter: f64,
        verse_start: f64,
        verse_end: Option<f64>,
        translation: Option<&str>,
    ) -> Result<Vec<ScriptureVerse>> {
        tracing::info!(book, chapter, verse_start, ?verse_end, "Scripture lookup");
        let Some(db) = self.db() else {
            return Ok(Vec::new());
        };
        let translation = translation.map_or_else(|| self.default_translation(), str::to_owned);
        let rows = match verse_end.filter(|&end| truthy(Some(end))) {
            Some(end) => {
                db.get_verse_range(&translation, book, chapter, verse_start, end)
                    .await?
            }
            None => db
                .get_verse(&translation, book, chapter, verse_start)
                .await?
                .into_iter()
                .collect(),
        };
        Ok(rows
            .into_iter()
            .map(|v| to_verse(v.book_name, chapter, v.verse as f64, v.text))
            .collect())
    }

    /// The translation picker's options: the registry in order, then any
    /// local translation it does not know. `online` asks the attached
    /// API.Bible source which ids it authorizes; startup passes `false`, so
    /// launch never waits on the network. With `strict`, that failure is
    /// returned instead of logged.
    pub async fn get_translation_options(
        &self,
        online: bool,
        strict: bool,
    ) -> Result<Vec<ScriptureTranslationOption>> {
        let (db, api) = {
            let state = self.state();
            (state.db.clone(), state.api.clone())
        };
        let local_rows = match db {
            Some(db) => db.get_all_translations().await?,
            None => Vec::new(),
        };
        let local: HashSet<String> = local_rows.iter().map(|row| row.id.to_uppercase()).collect();
        if let Some(api) = api.filter(|_| online) {
            match api.authorized_translations().await {
                Ok(ids) => {
                    tracing::info!(
                        count = ids.len(),
                        "[Scripture] API.Bible translations authorized"
                    );
                    self.state().api_translation_ids = ids;
                }
                Err(error) => {
                    tracing::warn!(%error, "[Scripture] Unable to load API.Bible translations");
                    if strict {
                        return Err(error.into());
                    }
                }
            }
        }
        let api_ids = self.state().api_translation_ids.clone();
        let mut seen = HashSet::new();
        let mut options = Vec::new();
        let mut push = |id: &str, name: &str, access: TranslationAccess| {
            let key = id.to_uppercase();
            if !seen.insert(key.clone()) {
                return;
            }
            let pack = get_downloadable_pack(&key);
            options.push(ScriptureTranslationOption {
                available: local.contains(&key) || api_ids.contains(&key),
                requires_api_key: access == TranslationAccess::Api && !local.contains(&key),
                downloadable: Some(pack.is_some()),
                download_approx: pack.map(|pack| pack.approx_label.to_owned()),
                id: key,
                name: name.to_owned(),
                access,
            });
        };
        for (id, name, access) in translation_catalog_entries() {
            push(id, name, access);
        }
        for row in &local_rows {
            let name = if row.name.is_empty() {
                &row.id
            } else {
                &row.name
            };
            push(&row.id, name, TranslationAccess::Local);
        }
        Ok(options)
    }
}
