//! `.kairo` files opened from Finder or Explorer. They can arrive before the
//! window can show them (macOS delivers the launching file before ready), so
//! they queue until the review screen says it is ready (`transfer:ready`).
//! Each file previews on its own; a bad file reports its error with its name.

use crate::{TransferService, lock};
use kairo_domain::kairo_bundle::TransferPreview;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What `transfer:opened` carries.
#[derive(Clone, Debug, PartialEq)]
pub enum KairoOpened {
    Preview(Box<TransferPreview>),
    Error { error: String, file_name: String },
}

impl KairoOpened {
    /// The JSON the renderer received: the preview, or `{ error, fileName }`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Self::Preview(p) => kairo_domain::json::to_js_value(p).unwrap_or_default(),
            Self::Error { error, file_name } => {
                serde_json::json!({ "error": error, "fileName": file_name })
            }
        }
    }
}

type Target = Arc<dyn Fn(KairoOpened) + Send + Sync>;

#[derive(Default)]
struct Inner {
    pending: VecDeque<PathBuf>,
    target: Option<Target>,
    flushing: bool,
}

#[derive(Clone)]
pub struct KairoOpenQueue {
    inner: Arc<Mutex<Inner>>,
    transfer: TransferService,
}

/// `argv` entries ending in `.kairo` (any case) that exist on disk.
pub fn kairo_paths_in(argv: &[String]) -> Vec<PathBuf> {
    argv.iter()
        .filter(|arg| arg.to_lowercase().ends_with(".kairo") && Path::new(arg).exists())
        .map(PathBuf::from)
        .collect()
}

impl KairoOpenQueue {
    pub fn new(transfer: TransferService) -> Self {
        Self {
            inner: Arc::default(),
            transfer,
        }
    }

    /// `openKairoFiles`: queue and deliver if the screen is ready.
    pub async fn open(&self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        lock(&self.inner).pending.extend(paths);
        self.flush().await;
    }

    /// `transfer:ready`: the review screen (re)attached; deliver the queue to it.
    pub async fn ready(&self, target: impl Fn(KairoOpened) + Send + Sync + 'static) {
        lock(&self.inner).target = Some(Arc::new(target));
        self.flush().await;
    }

    /// The window went away; files queue again until the next `ready`.
    pub fn detach(&self) {
        lock(&self.inner).target = None;
    }

    pub fn pending(&self) -> usize {
        lock(&self.inner).pending.len()
    }

    async fn flush(&self) {
        {
            let mut inner = lock(&self.inner);
            // One drain at a time keeps the files in order.
            if inner.flushing || inner.target.is_none() {
                return;
            }
            inner.flushing = true;
        }
        loop {
            let (path, target) = {
                let mut inner = lock(&self.inner);
                let Some(target) = inner.target.clone() else {
                    inner.flushing = false;
                    return;
                };
                let Some(path) = inner.pending.pop_front() else {
                    inner.flushing = false;
                    return;
                };
                (path, target)
            };
            let message = match self.transfer.preview_file(&path).await {
                Ok(preview) => KairoOpened::Preview(Box::new(preview)),
                Err(error) => KairoOpened::Error {
                    error: error.to_string(),
                    file_name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                },
            };
            target(message);
        }
    }
}
