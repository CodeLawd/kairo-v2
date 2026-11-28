//! The Imagine page's backend: Imagine's draft store, the operator's OpenAI
//! key in the keychain, and Save to Media. Generating and refining never touch
//! the media library or the outputs; only `SaveToMedia` imports, and it
//! never goes live.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kairo_imagine::{Action, ImagineError, ImagineStore, Kind, OwnKeyTransport, work};
use kairo_store::secrets::{CREDENTIALS_NOT_SAVED_NOTICE, SecureStore};
use ui_imagine::{
    Attachment, FeedSink, ImagineAnswer, ImagineBackend, ImagineCall, ImagineFeed, ImagineSnapshot,
    Reply,
};

use crate::services::ApplicationServices;

const KEY_NAME: &str = "openai-api-key";
/// Source text sent to the assistant, at most.
const MAX_ATTACHMENT_CHARS: usize = 30_000;
const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;

pub struct ServicesImagine {
    inner: Arc<Inner>,
}

struct Inner {
    services: Arc<ApplicationServices>,
    store: Result<Mutex<ImagineStore>, ImagineError>,
    /// The key, once read or entered: keychain reads can prompt and block.
    key: Mutex<Option<String>>,
    /// Kept only for this launch when the keychain refused it.
    unsaved: Mutex<bool>,
    cancels: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<()>>>,
    /// Where job progress goes (the page).
    feed: Mutex<Option<FeedSink>>,
}

impl ServicesImagine {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        let store = ImagineStore::open(services.user_data()).map(Mutex::new);
        if let Err(error) = &store {
            tracing::error!(%error, "[Imagine] store unavailable");
        }
        Self {
            inner: Arc::new(Inner {
                services,
                store,
                key: Mutex::default(),
                unsaved: Mutex::default(),
                cancels: Mutex::default(),
                feed: Mutex::default(),
            }),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn store(&self) -> Result<&Mutex<ImagineStore>, String> {
        self.store.as_ref().map_err(|e| e.to_string())
    }

    fn secrets(&self) -> SecureStore<'_> {
        SecureStore::new(
            self.services.user_data(),
            self.services.protector.as_ref(),
            "Kairo",
        )
    }

    /// The key, read once per launch. The account's key wins, so a key
    /// saved on one device works on every device the person signs in on; a
    /// key only this device has (saved before keys synced) is sent up.
    async fn key(self: &Arc<Self>) -> Option<String> {
        if let Some(key) = lock(&self.key).clone() {
            return Some(key);
        }
        let inner = self.clone();
        let local = tokio::task::spawn_blocking(move || inner.secrets().read(KEY_NAME))
            .await
            .ok()
            .flatten()
            .filter(|k| !k.is_empty());
        let key = match self.services.cloud.imagine_key().await {
            Ok(Some(account)) => {
                if local.as_deref() != Some(account.as_str()) {
                    let (inner, value) = (self.clone(), account.clone());
                    let saved = tokio::task::spawn_blocking(move || {
                        inner.secrets().write(KEY_NAME, &value)
                    })
                    .await
                    .unwrap_or(false);
                    *lock(&self.unsaved) = !saved;
                }
                Some(account)
            }
            Ok(None) => {
                // shortcut: a key removed on another device comes back from one
                // that still holds it; a per-key "removed" marker would stop that.
                if let Some(local) = &local {
                    self.push_key(Some(local.clone()));
                }
                local
            }
            // Offline or the API is down: this device's copy still works.
            Err(error) => {
                tracing::warn!(%error, "[Imagine] account key unavailable");
                local
            }
        };
        *lock(&self.key) = key.clone();
        key
    }

    /// Saves (or forgets) the key on the account, in the background.
    fn push_key(self: &Arc<Self>, key: Option<String>) {
        let cloud = self.services.cloud.clone();
        tokio::spawn(async move {
            if let Err(error) = cloud.set_imagine_key(key.as_deref()).await {
                tracing::warn!(%error, "[Imagine] could not sync the key to the account");
            }
        });
    }

    async fn snapshot(self: &Arc<Self>) -> Result<ImagineAnswer, String> {
        let connected = self.key().await.is_some();
        let projects = lock(self.store()?).list().map_err(|e| e.to_string())?;
        let notice =
            (connected && *lock(&self.unsaved)).then(|| CREDENTIALS_NOT_SAVED_NOTICE.into());
        Ok(ImagineAnswer::Snapshot(ImagineSnapshot {
            connected,
            projects,
            notice,
        }))
    }

    async fn connect(self: &Arc<Self>, key: String) -> Result<ImagineAnswer, String> {
        OwnKeyTransport::new(key.clone())
            .verify()
            .await
            .map_err(|e| e.to_string())?;
        let inner = self.clone();
        let value = key.clone();
        let saved = tokio::task::spawn_blocking(move || inner.secrets().write(KEY_NAME, &value))
            .await
            .unwrap_or(false);
        *lock(&self.unsaved) = !saved;
        *lock(&self.key) = Some(key.clone());
        self.push_key(Some(key));
        self.snapshot().await
    }

    async fn disconnect(self: &Arc<Self>) -> Result<ImagineAnswer, String> {
        *lock(&self.key) = None;
        *lock(&self.unsaved) = false;
        let inner = self.clone();
        let _ = tokio::task::spawn_blocking(move || inner.secrets().clear(KEY_NAME)).await;
        // One account: disconnecting here disconnects every device.
        self.push_key(None);
        self.snapshot().await
    }

    /// Runs one message to the assistant until it ends or is cancelled:
    /// plan, tell the page, then make what was planned.
    async fn job(self: &Arc<Self>, call: ImagineCall) -> Result<ImagineAnswer, String> {
        let ImagineCall::Send {
            job,
            project,
            from,
            message,
            style,
            attachment,
            model,
            quality,
        } = call
        else {
            return Err(ImagineError::Missing.to_string());
        };
        let key = self
            .key()
            .await
            .ok_or_else(|| ImagineError::NotConnected.to_string())?;
        let transport = OwnKeyTransport::new(key);
        let store = self.store()?;
        let current = match &project {
            Some(id) => Some(lock(store).get(id).map_err(|e| e.to_string())?),
            None => None,
        };
        let from = from.or_else(|| current.as_ref().map(|p| p.selected.clone()));
        let (tx, rx) = tokio::sync::oneshot::channel();
        lock(&self.cancels).insert(job, tx);
        let feed = lock(&self.feed).clone();
        let look = work::Look { model, quality };
        let work = async {
            let mut asked = match &style {
                Some(style) => format!("{message}\n\nStyle: {style}"),
                None => message.clone(),
            };
            if let Some(a) = &attachment {
                asked.push_str(&format!("\n\nAttached source ({}):\n{}", a.name, a.text));
            }
            let thought = |text: &str| {
                if let Some(feed) = &feed {
                    feed(ImagineFeed::Thought {
                        job,
                        text: text.to_owned(),
                    });
                }
            };
            let mut plan = work::plan(&transport, current.as_ref(), &asked, &thought).await?;
            let mut action = plan.action;
            // When the operator plainly asked for movement and the assistant
            // only planned a picture, the picture still moves.
            if plan.motion.is_none() && matches!(action, Action::Image | Action::EditImage) {
                plan.motion = kairo_imagine::motion::motion_hint(&message).map(str::to_owned);
            }
            // Inside an image, "make an image" means change it.
            if action == Action::Image && current.as_ref().is_some_and(|p| p.kind == Kind::Image) {
                action = Action::EditImage;
            }
            if let Some(feed) = &feed {
                feed(ImagineFeed::Planned {
                    job,
                    reply: plan.reply.clone(),
                    action,
                    slides: plan.deck.as_ref().map_or(0, |d| d.slides.len()),
                    titles: deck_titles(&plan),
                });
            }
            let reply = Some(plan.reply.clone());
            let name = plan.title.trim().to_owned();
            // A creation still named after its first message takes the
            // assistant's name now.
            if let Some(p) = current.as_ref().filter(|p| p.has_default_title())
                && !name.is_empty()
                && action != Action::Ask
            {
                lock(store).rename(&p.id, &name)?;
            }
            let prompt = plan.image_prompt.clone().unwrap_or_default();
            let progress = |step| {
                if let Some(feed) = &feed {
                    feed(match step {
                        work::Step::Painting { done, total } => {
                            ImagineFeed::Progress { job, done, total }
                        }
                        work::Step::Designing { done, total } => {
                            ImagineFeed::Designing { job, done, total }
                        }
                    });
                }
            };
            let empty = kairo_imagine::DeckPlan {
                title: String::new(),
                rhythm: Vec::new(),
                theme: Default::default(),
                background_prompt: String::new(),
                slides: Vec::new(),
            };
            let deck_plan = plan.deck.as_ref().unwrap_or(&empty);
            let answer = match (action, &project, &from) {
                (Action::Ask, _, _) => Ok(ImagineAnswer::Reply(plan.reply.clone())),
                (Action::Motion, Some(p), Some(from)) => {
                    let motion = plan.motion.clone().unwrap_or_else(|| "dust".into());
                    match self
                        .save_to_media(p.clone(), from.clone(), Some(motion.clone()))
                        .await
                    {
                        Ok(ImagineAnswer::Project(project)) => Ok(ImagineAnswer::Moved {
                            reply: plan.reply.clone(),
                            motion,
                            project,
                            made: false,
                        }),
                        Ok(other) => Ok(other),
                        Err(e) => Err(ImagineError::Media(e)),
                    }
                }
                (Action::EditImage, Some(p), Some(from)) => {
                    work::refine(store, &transport, p, from, &message, &prompt, reply, look)
                        .await
                        .map(ImagineAnswer::Project)
                }
                (Action::EditSlides, Some(p), Some(from)) => work::deck(
                    store,
                    &transport,
                    Some((p, from)),
                    &message,
                    reply,
                    deck_plan,
                    look,
                    &progress,
                )
                .await
                .map(ImagineAnswer::Project),
                (Action::Deck | Action::EditSlides, _, _) => work::deck(
                    store, &transport, None, &message, reply, deck_plan, look, &progress,
                )
                .await
                .map(ImagineAnswer::Project),
                _ => {
                    let made =
                        work::generate(store, &transport, &message, &prompt, reply, look).await?;
                    // Named by the assistant, so saved files read well.
                    if name.is_empty() {
                        Ok(ImagineAnswer::Project(made))
                    } else {
                        lock(store)
                            .rename(&made.id, &name)
                            .map(ImagineAnswer::Project)
                    }
                }
            }?;
            // A picture asked for moving is saved to Media moving straight away.
            match (answer, plan.motion.clone()) {
                (ImagineAnswer::Project(made), Some(motion))
                    if matches!(action, Action::Image | Action::EditImage) =>
                {
                    let saved = self
                        .save_to_media(made.id.clone(), made.selected.clone(), Some(motion.clone()))
                        .await;
                    Ok(match saved {
                        Ok(ImagineAnswer::Project(project)) => ImagineAnswer::Moved {
                            reply: plan.reply,
                            motion,
                            project,
                            made: true,
                        },
                        // The picture is made either way; Save to Media can be tried again.
                        other => {
                            if let Err(error) = other {
                                tracing::warn!(%error, "[Imagine] saving the moving version failed");
                            }
                            ImagineAnswer::Project(made)
                        }
                    })
                }
                (answer, _) => Ok(answer),
            }
        };
        // Dropping `work` on cancel drops the request; nothing was recorded.
        let result = tokio::select! {
            result = work => result,
            _ = rx => Err(ImagineError::Cancelled),
        };
        lock(&self.cancels).remove(&job);
        result.map_err(|e| e.to_string())
    }

    /// Picks a document and reads its text for the assistant: plain text and
    /// Markdown as they are, Word and PDF through the sermon-notes reader.
    async fn pick_attachment(self: &Arc<Self>) -> Result<ImagineAnswer, String> {
        use kairo_core::dialogs::{DialogKind, dialog_request};
        let Some(path) = self
            .services
            .ui
            .dialog(dialog_request(DialogKind::SermonNotes, None))
            .await?
            .into_iter()
            .next()
        else {
            return Ok(ImagineAnswer::Done);
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Attachment".into());
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let text = match ext.as_str() {
            "txt" | "md" | "markdown" => {
                let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
                if bytes.len() > MAX_ATTACHMENT_BYTES {
                    return Err("Choose a text file smaller than 5 MB.".into());
                }
                String::from_utf8_lossy(&bytes).into_owned()
            }
            "docx" | "pdf" => {
                kairo_documents::script_import::import_script(path)
                    .await
                    .map_err(|e| e.to_string())?
                    .text
            }
            _ => return Err("Attach a .txt, .md, .pdf or .docx file.".into()),
        };
        let text = text.trim();
        if text.is_empty() {
            return Err("That file has no text to read.".into());
        }
        let truncated = text.chars().count() > MAX_ATTACHMENT_CHARS;
        let text: String = text.chars().take(MAX_ATTACHMENT_CHARS).collect();
        Ok(ImagineAnswer::Attachment(Attachment {
            name,
            text,
            truncated,
        }))
    }

    /// Writes the deck as a .pptx in Downloads (a new name if one exists)
    /// and shows it in Finder.
    async fn download_pptx(
        self: &Arc<Self>,
        project: String,
        version: String,
    ) -> Result<ImagineAnswer, String> {
        let inner = self.clone();
        let path = self
            .services
            .core
            .blocking(move || -> Result<PathBuf, String> {
                let store = inner.store()?;
                let (title, bytes) =
                    work::export_pptx(store, &project, &version).map_err(|e| e.to_string())?;
                let dir = dirs::download_dir()
                    .or_else(dirs::home_dir)
                    .ok_or("There is no Downloads folder.")?;
                let stem = media_name(&title, 1);
                let stem = stem.trim_end_matches(" v1.png");
                let mut path = dir.join(format!("{stem}.pptx"));
                let mut n = 2;
                while path.exists() {
                    path = dir.join(format!("{stem} {n}.pptx"));
                    n += 1;
                }
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                Ok(path)
            })
            .await
            .map_err(|e| e.to_string())??;
        crate::handlers::open_path(&path, true)?;
        Ok(ImagineAnswer::File(path))
    }

    /// Adds a deck version to Slides, named after the creation.
    async fn save_to_slides(
        self: &Arc<Self>,
        project: String,
        version: String,
    ) -> Result<ImagineAnswer, String> {
        let (title, pages) = {
            let store = lock(self.store()?);
            let p = store.get(&project).map_err(|e| e.to_string())?;
            let v = p
                .version(&version)
                .ok_or_else(|| ImagineError::Missing.to_string())?;
            let pages = v
                .slides
                .iter()
                .map(|s| store.read_file(&s.file))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            (p.title.clone(), pages)
        };
        let documents = self.services.documents.clone();
        self.services
            .core
            .blocking(move || documents.create_from_images(&title, &pages))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let marked = lock(self.store()?).mark_saved(&project, &version);
        marked
            .map(ImagineAnswer::Project)
            .map_err(|e| e.to_string())
    }

    async fn save_to_media(
        self: &Arc<Self>,
        project: String,
        version: String,
        motion: Option<String>,
    ) -> Result<ImagineAnswer, String> {
        let preset = motion
            .as_deref()
            .and_then(kairo_imagine::motion::MotionPreset::by_key);
        let (source, name, look) = {
            let store = lock(self.store()?);
            let p = store.get(&project).map_err(|e| e.to_string())?;
            let index = p
                .versions
                .iter()
                .position(|v| v.id == version)
                .ok_or_else(|| ImagineError::Missing.to_string())?;
            let v = &p.versions[index];
            let look = work::Look {
                model: v.model.unwrap_or_default(),
                quality: v.quality,
            };
            (
                store.file_path(&v.file),
                media_name(&p.title, index + 1),
                look,
            )
        };
        let bytes = std::fs::read(&source).map_err(|e| e.to_string())?;
        // Parallax: one more request cuts the foreground out on transparency.
        let mut layers = Vec::new();
        if preset.is_some_and(|p| p.depth) {
            let key = self
                .key()
                .await
                .ok_or_else(|| ImagineError::NotConnected.to_string())?;
            let transport = OwnKeyTransport::new(key);
            let layer = work::depth_layer(self.store()?, &transport, &project, &version, look)
                .await
                .map_err(|e| e.to_string())?;
            layers.push(layer.to_string_lossy().into_owned());
        }
        self.import(&name, &bytes, preset.map(|p| p.value(&layers)), &version)
            .await?;
        let marked = lock(self.store()?).mark_saved(&project, &version);
        marked
            .map(ImagineAnswer::Project)
            .map_err(|e| e.to_string())
    }

    /// A motion loop: drawn here, no image request, saved moving.
    async fn save_loop(self: &Arc<Self>, key: String) -> Result<ImagineAnswer, String> {
        use kairo_imagine::motion::{MotionLoop, MotionPreset, render_loop};
        let l = MotionLoop::by_key(&key).ok_or_else(|| ImagineError::Missing.to_string())?;
        let bytes = tokio::task::spawn_blocking(move || render_loop(&l, (1920, 1080)))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let motion = MotionPreset::by_key(l.preset).map(|p| p.value(&[]));
        self.import(&format!("{} loop.png", l.title), &bytes, motion, &key)
            .await?;
        Ok(ImagineAnswer::Done)
    }

    /// Imports `bytes` as `name` into Imagine's playlist (made the first
    /// time), then gives the new item its `motion`. Never goes live.
    async fn import(
        self: &Arc<Self>,
        name: &str,
        bytes: &[u8],
        motion: Option<serde_json::Value>,
        tag: &str,
    ) -> Result<(), String> {
        let actions = self.services.actions();
        let no_folder = lock(&self.services.media).library().folder.is_empty();
        if no_folder && actions.choose_media_folder().await?.folder.is_empty() {
            return Err("Choose a media folder to save into.".into());
        }
        // The media library names items after their file, so import a copy
        // with a readable name.
        let staging = std::env::temp_dir().join(format!("kairo-imagine-{}", uuid_suffix(tag)));
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let copy = staging.join(name);
        std::fs::write(&copy, bytes).map_err(|e| e.to_string())?;
        // Into Imagine's own playlist, made the first time, so saved
        // pictures are easy to find in the Media dock.
        let (playlist, before) = {
            let mut media = lock(&self.services.media);
            let find = |l: &kairo_domain::media::MediaLibrary| {
                l.playlists
                    .iter()
                    .find(|p| p.name == kairo_domain::media::IMAGINE_PLAYLIST)
                    .map(|p| p.id.clone())
            };
            let library = media.library();
            let before: std::collections::HashSet<String> =
                library.items.iter().map(|i| i.id.clone()).collect();
            let playlist = match find(&library) {
                Some(id) => Some(id),
                None => find(&media.create_playlist(kairo_domain::media::IMAGINE_PLAYLIST)),
            };
            (playlist, before)
        };
        let imported = actions.import_media_files(playlist, vec![copy]).await;
        let _ = std::fs::remove_dir_all(&staging);
        let library = imported?;
        if let Some(motion) = motion {
            let new = library.items.iter().find(|i| !before.contains(&i.id));
            if let Some(item) = new {
                let mut patch = serde_json::Map::new();
                patch.insert("motion".into(), motion);
                actions.set_media_playback(&item.id, &patch, true)?;
            }
        }
        Ok(())
    }
}

fn uuid_suffix(id: &str) -> String {
    id.chars()
        .filter(char::is_ascii_alphanumeric)
        .take(12)
        .collect()
}

/// The planned slides' titles.
fn deck_titles(plan: &kairo_imagine::Plan) -> Vec<String> {
    let mut titles = Vec::new();
    if let Some(deck) = &plan.deck {
        // The rhythm map: each slide's layout beside its title.
        for slide in &deck.slides {
            titles.push(format!("{} · {}", slide.layout.name(), slide.title));
        }
    }
    titles
}

/// `Sunrise over hills v2.png`, without characters a file name can't hold.
fn media_name(title: &str, version: usize) -> String {
    let clean: String = title
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '…'
            )
        })
        .collect();
    let clean = clean.trim();
    let base = if clean.is_empty() { "Imagine" } else { clean };
    format!("{base} v{version}.png")
}

impl ImagineBackend for ServicesImagine {
    fn call(&self, call: ImagineCall, reply: Reply) {
        let inner = self.inner.clone();
        self.inner.services.spawn(async move {
            let answer = match call {
                ImagineCall::Load => inner.snapshot().await,
                ImagineCall::Connect(key) => inner.connect(key).await,
                ImagineCall::Disconnect => inner.disconnect().await,
                ImagineCall::Send { .. } => inner.job(call).await,
                ImagineCall::PickAttachment => inner.pick_attachment().await,
                ImagineCall::DownloadPptx { project, version } => {
                    inner.download_pptx(project, version).await
                }
                ImagineCall::SaveToSlides { project, version } => {
                    inner.save_to_slides(project, version).await
                }
                ImagineCall::Cancel(job) => {
                    if let Some(tx) = lock(&inner.cancels).remove(&job) {
                        let _ = tx.send(());
                    }
                    Ok(ImagineAnswer::Done)
                }
                ImagineCall::Select { project, version } => inner.store().and_then(|s| {
                    lock(s)
                        .select(&project, &version)
                        .map(ImagineAnswer::Project)
                        .map_err(|e| e.to_string())
                }),
                ImagineCall::SaveToMedia {
                    project,
                    version,
                    motion,
                } => inner.save_to_media(project, version, motion).await,
                ImagineCall::SaveLoop(key) => inner.save_loop(key).await,
                ImagineCall::Delete(project) => inner.store().and_then(|s| {
                    lock(s)
                        .delete(&project)
                        .map(|_| ImagineAnswer::Done)
                        .map_err(|e| e.to_string())
                }),
            };
            reply(answer);
        });
    }

    fn listen(&self, sink: FeedSink) {
        *lock(&self.inner.feed) = Some(sink.clone());
        // The account's key arrives with the account: the page asks again
        // whenever who is signed in changes (restored after launch, or a
        // sign in or out).
        let services = self.inner.services.clone();
        let inner = self.inner.clone();
        services.clone().runtime().spawn(async move {
            let mut sessions = services
                .core
                .watch::<kairo_domain::cloud::SessionSnapshot>();
            let mut who = None;
            while let Some(s) = sessions.next().await {
                let now = s.user.as_ref().map(|u| (u.id.clone(), u.email_verified));
                if now != who {
                    let signed_out = who.is_some() && now.is_none();
                    who = now;
                    // Another person's key must not linger in this launch.
                    *lock(&inner.key) = None;
                    if signed_out {
                        // Or on this computer: the next person to sign in
                        // would otherwise have it sent up to their account.
                        let inner = inner.clone();
                        let _ =
                            tokio::task::spawn_blocking(move || inner.secrets().clear(KEY_NAME))
                                .await;
                    }
                    sink(ImagineFeed::AccountChanged);
                }
            }
        });
    }

    fn image_path(&self, file: &str) -> PathBuf {
        match &self.inner.store {
            Ok(store) => lock(store).file_path(file),
            Err(_) => PathBuf::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn media_names_are_readable_file_names() {
        assert_eq!(
            super::media_name("Sunrise: over / hills…", 2),
            "Sunrise over  hills v2.png"
        );
        assert_eq!(super::media_name("  ", 1), "Imagine v1.png");
    }
}
