//! `.kairo` export and import. The service gathers what to export from the
//! authoritative stores, so a caller only names which songs or list. On
//! import the parsed bundle stays here under a token while the operator
//! reviews it; the caller sends back decisions, not data.

use crate::{Clock, Error, Result, SetlistService, lock, new_id};
use kairo_domain::kairo_bundle::{
    KAIRO_BUNDLE_EXT, KAIRO_BUNDLE_MAX_BYTES, KairoBundle, PlaylistImportChoice, SongImportChoice,
    TransferCommitRequest, TransferCommitResult, TransferExportRequest, TransferExportResult,
    TransferPlaylistPreview, TransferPreview, TransferSetlistPreview, TransferSkippedFile,
    TransferSongPreview, bundle_file_name, merge_kairo_bundles, parse_kairo_bundle,
    playlist_bundle, serialize_bundle, setlist_bundle, songs_bundle,
};
use kairo_domain::lyrics::duplicate::find_duplicate;
use kairo_domain::lyrics::{DuplicateMatch, DuplicateReason, LyricsSong};
use kairo_domain::scripture::SermonPlan;
use kairo_lyrics::LyricsService;
use kairo_orchestrator::live_plan::LivePlanService;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The native dialogs, owned by the app shell.
pub trait FileDialogs: Send + Sync {
    /// "Export to a Kairo file", filter `kairo`. `None` when cancelled.
    fn save_kairo<'a>(&'a self, default_name: &'a str) -> BoxFuture<'a, Option<PathBuf>>;
    /// "Import Kairo files", several files, filter `kairo` and `json`.
    /// Empty when cancelled.
    fn open_kairo(&self) -> BoxFuture<'_, Vec<PathBuf>>;
}

/// Songs written so far during a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitProgress {
    pub done: usize,
    pub total: usize,
}

struct Pending {
    bundle: KairoBundle,
    file_name: String,
}

#[derive(Clone)]
pub struct TransferService {
    lyrics: Arc<LyricsService>,
    setlist: SetlistService,
    plans: LivePlanService,
    app_version: String,
    clock: Clock,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl TransferService {
    pub fn new(
        lyrics: Arc<LyricsService>,
        setlist: SetlistService,
        plans: LivePlanService,
        app_version: impl Into<String>,
        clock: Clock,
    ) -> Self {
        Self {
            lyrics,
            setlist,
            plans,
            app_version: app_version.into(),
            clock,
            pending: Arc::default(),
        }
    }

    // ─── Export ─────────────────────────────────────────────────────────────

    pub async fn build_bundle(&self, request: &TransferExportRequest) -> Result<KairoBundle> {
        let version = Some(self.app_version.clone());
        let now = (self.clock)();
        let library = self.lyrics.get_library().await?;
        let by_id: HashMap<&str, &LyricsSong> =
            library.iter().map(|s| (s.id.as_str(), s)).collect();
        let pick = |ids: &[String]| -> Vec<LyricsSong> {
            ids.iter()
                .filter_map(|id| by_id.get(id.as_str()).map(|s| (*s).clone()))
                .collect()
        };
        Ok(match request {
            TransferExportRequest::Library => {
                if library.is_empty() {
                    return Err(Error::User("The song library is empty."));
                }
                songs_bundle(
                    format!("All songs ({})", library.len()),
                    library.clone(),
                    version,
                    now,
                )
            }
            TransferExportRequest::Songs { song_ids, name } => {
                let songs = pick(song_ids);
                if songs.is_empty() {
                    return Err(Error::User("Choose at least one song to export."));
                }
                let name = name
                    .as_deref()
                    .map(kairo_domain::js::trim)
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        if songs.len() == 1 {
                            songs[0].title.clone()
                        } else {
                            format!("{} songs", songs.len())
                        }
                    });
                songs_bundle(name, songs, version, now)
            }
            TransferExportRequest::Setlist { list_id } => {
                let list = self
                    .setlist
                    .snapshot()
                    .lists
                    .into_iter()
                    .find(|l| &l.id == list_id)
                    .ok_or(Error::User("That playlist no longer exists."))?;
                setlist_bundle(list.name, pick(&list.song_ids), version, now)
            }
            TransferExportRequest::Playlist { plan_id } => {
                let plan = self
                    .plans
                    .plan(plan_id)
                    .ok_or(Error::User("That playlist no longer exists."))?;
                playlist_bundle(plan, version, now)
            }
        })
    }

    /// `transfer:export`. The bundle is built (and its errors raised) before
    /// the dialog opens, as in Electron.
    pub async fn export(
        &self,
        request: TransferExportRequest,
        dialogs: &dyn FileDialogs,
    ) -> Result<TransferExportResult> {
        let bundle = self.build_bundle(&request).await?;
        let Some(path) = dialogs.save_kairo(&bundle_file_name(&bundle.name)).await else {
            return Ok(TransferExportResult {
                saved: false,
                file_name: None,
            });
        };
        let path = self.write_bundle(&bundle, path).await?;
        Ok(TransferExportResult {
            saved: true,
            file_name: Some(base_name(&path)),
        })
    }

    /// Writes `bundle` as UTF-8 JSON, adding `.kairo` when the name lacks it.
    pub async fn write_bundle(&self, bundle: &KairoBundle, path: PathBuf) -> Result<PathBuf> {
        let path = if path.to_string_lossy().ends_with(KAIRO_BUNDLE_EXT) {
            path
        } else {
            PathBuf::from(format!("{}{KAIRO_BUNDLE_EXT}", path.to_string_lossy()))
        };
        let json = serialize_bundle(bundle).map_err(|e| Error::Message(e.to_string()))?;
        // Atomic, so a failed overwrite keeps the operator's earlier file.
        let target = path.clone();
        tokio::task::spawn_blocking(move || {
            kairo_store::fs_atomic::write_atomic(&target, json.as_bytes())
        })
        .await
        .map_err(std::io::Error::other)??;
        tracing::info!(
            kind = ?bundle.kind,
            songs = bundle.songs.len(),
            playlist_items = bundle.playlist.as_ref().map_or(0, |p| p.items.len()),
            "[Transfer] Exported"
        );
        Ok(path)
    }

    // ─── Import ─────────────────────────────────────────────────────────────

    /// `transfer:pickAndPreview`. `None` when the dialog is cancelled.
    pub async fn pick_and_preview(
        &self,
        dialogs: &dyn FileDialogs,
    ) -> Result<Option<TransferPreview>> {
        let paths = dialogs.open_kairo().await;
        if paths.is_empty() {
            return Ok(None);
        }
        self.preview_files(&paths).await.map(Some)
    }

    async fn read_bundle(&self, path: &Path) -> std::result::Result<KairoBundle, String> {
        let info = tokio::fs::metadata(path)
            .await
            .map_err(|e| io_message(&e, path))?;
        if info.len() > KAIRO_BUNDLE_MAX_BYTES as u64 {
            return Err("This file is too large to be a Kairo export.".into());
        }
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|e| io_message(&e, path))?;
        // Node's `utf8` decoding: invalid bytes become U+FFFD.
        let raw = String::from_utf8_lossy(&bytes);
        parse_kairo_bundle(&raw, (self.clock)()).map_err(str::to_owned)
    }

    pub async fn preview_file(&self, path: &Path) -> Result<TransferPreview> {
        self.preview_files(&[path.to_owned()]).await
    }

    /// One review for any number of files. A file that cannot be read is
    /// listed and skipped rather than failing the batch.
    pub async fn preview_files(&self, paths: &[PathBuf]) -> Result<TransferPreview> {
        let mut bundles = Vec::new();
        let mut skipped = Vec::new();
        for path in paths {
            match self.read_bundle(path).await {
                Ok(bundle) => bundles.push(bundle),
                Err(error) => skipped.push(TransferSkippedFile {
                    file_name: base_name(path),
                    error,
                }),
            }
        }
        if bundles.is_empty() {
            let first = skipped
                .first()
                .map_or_else(|| "No files to import.".to_owned(), |s| s.error.clone());
            return Err(Error::Message(if paths.len() == 1 {
                first
            } else {
                format!("None of the {} files could be read. {first}", paths.len())
            }));
        }
        let bundle = merge_kairo_bundles(bundles).map_err(Error::User)?;
        let file_name = if paths.len() == 1 {
            base_name(&paths[0])
        } else {
            format!("{} files", paths.len())
        };
        let library = self.lyrics.get_library().await?;
        let by_id: HashMap<&str, &LyricsSong> =
            library.iter().map(|s| (s.id.as_str(), s)).collect();
        let songs = bundle
            .songs
            .iter()
            .map(|song| TransferSongPreview {
                id: song.id.clone(),
                title: song.title.clone(),
                artist: song.artist.clone(),
                section_count: song.sections.len(),
                // Same id means it came from this library: the strongest signal.
                duplicate: match by_id.get(song.id.as_str()) {
                    Some(same) => Some(DuplicateMatch {
                        song_id: same.id.clone(),
                        title: same.title.clone(),
                        artist: same.artist.clone(),
                        reason: DuplicateReason::SameSong,
                    }),
                    None => find_duplicate(song, &library),
                },
            })
            .collect();
        let preview = TransferPreview {
            token: new_id(),
            file_name: file_name.clone(),
            kind: bundle.kind,
            name: bundle.name.clone(),
            exported_at: bundle.exported_at,
            songs,
            setlist: bundle.setlist.as_ref().map(|s| TransferSetlistPreview {
                name: s.name.clone(),
                song_count: s.song_ids.len(),
            }),
            playlist: bundle.playlist.as_ref().map(|p| TransferPlaylistPreview {
                title: p.title.clone(),
                item_count: p.items.len(),
                exists: self.plans.plan(&p.id).is_some(),
            }),
            skipped_files: (!skipped.is_empty()).then_some(skipped),
        };
        lock(&self.pending).insert(preview.token.clone(), Pending { bundle, file_name });
        Ok(preview)
    }

    /// `transfer:discard`.
    pub fn discard(&self, token: &str) {
        lock(&self.pending).remove(token);
    }

    /// `transfer:commit`. Songs are written one at a time on the lyrics
    /// worker; `progress` reports each.
    pub async fn commit(
        &self,
        request: TransferCommitRequest,
        mut progress: impl FnMut(CommitProgress) + Send,
    ) -> Result<TransferCommitResult> {
        let entry = lock(&self.pending)
            .remove(&request.token)
            .ok_or(Error::User("This import has expired. Open the file again."))?;
        let bundle = entry.bundle;
        let library = self.lyrics.get_library().await?;
        let by_id: HashMap<&str, &LyricsSong> =
            library.iter().map(|s| (s.id.as_str(), s)).collect();
        let mut result = TransferCommitResult {
            added: 0,
            replaced: 0,
            skipped: 0,
            setlist_name: None,
            playlist_title: None,
        };
        // Bundle song id to the library id it ended up as (for the setlist).
        let mut landed: HashMap<String, String> = HashMap::new();
        let total = bundle.songs.len();

        for (index, song) in bundle.songs.iter().enumerate() {
            let choice = request
                .songs
                .get(&song.id)
                .copied()
                .unwrap_or(SongImportChoice::Skip);
            let matched: Option<&LyricsSong> = match by_id.get(song.id.as_str()) {
                Some(s) => Some(s),
                None => find_duplicate(song, &library)
                    .and_then(|d| by_id.get(d.song_id.as_str()).copied()),
            };
            match (choice, matched) {
                (SongImportChoice::Skip, m) => {
                    result.skipped += 1;
                    if let Some(m) = m {
                        landed.insert(song.id.clone(), m.id.clone());
                    }
                }
                (SongImportChoice::Replace, Some(m)) => {
                    let mut replacement = song.clone();
                    replacement.id = m.id.clone();
                    if self.lyrics.update_song(&m.id, replacement).await?.is_some() {
                        result.replaced += 1;
                        landed.insert(song.id.clone(), m.id.clone());
                    }
                }
                // 'add', 'keep-both', or a 'replace' whose target vanished: a
                // fresh id whenever the original is taken.
                _ => {
                    let fresh = by_id.contains_key(song.id.as_str());
                    let saved = self.lyrics.add_imported_song(song.clone(), fresh).await?;
                    result.added += 1;
                    landed.insert(song.id.clone(), saved.id);
                }
            }
            progress(CommitProgress {
                done: index + 1,
                total,
            });
        }

        if let Some(setlist) = bundle.setlist.as_ref().filter(|_| request.create_setlist) {
            let ids: Vec<String> = setlist
                .song_ids
                .iter()
                .filter_map(|id| landed.get(id).cloned())
                .filter(|id| !id.is_empty())
                .collect();
            result.setlist_name = Some(self.setlist.import_list(&setlist.name, &ids)?.name);
        }

        if let (Some(incoming), Some(choice)) = (bundle.playlist.clone(), request.playlist)
            && choice != PlaylistImportChoice::Skip
        {
            let exists = self.plans.plan(&incoming.id).is_some();
            let plan = if choice == PlaylistImportChoice::Replace || !exists {
                incoming
            } else {
                SermonPlan {
                    id: new_id(),
                    title: format!("{} (imported)", incoming.title),
                    created_at: (self.clock)(),
                    ..incoming
                }
            };
            // Saves and refreshes the live index in one step.
            let saved = self.plans.save_plan(plan);
            result.playlist_title = Some(saved.title);
        }

        tracing::info!(
            file = entry.file_name,
            kind = ?bundle.kind,
            added = result.added,
            replaced = result.replaced,
            skipped = result.skipped,
            "[Transfer] Imported"
        );
        Ok(result)
    }
}

/// Node's error text for the two failures a picked path realistically hits.
fn io_message(error: &std::io::Error, path: &Path) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            format!(
                "ENOENT: no such file or directory, stat '{}'",
                path.display()
            )
        }
        _ => error.to_string(),
    }
}
