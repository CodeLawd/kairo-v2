//! Transcript event recording for development replay. From
//! `src/main/services/scripture/transcript-recording.ts`, opened from
//! `KAIRO_STT_REPLAY_PATH`.
//!
//! An explicit local opt-in: the file is created exclusively with mode 0600
//! and never overwritten. Writes happen on their own thread and never block
//! live transcription; events dropped while the writer is behind are marked
//! with a `gap` line, which replay refuses.

use kairo_domain::transcript::{InterimResult, TranscriptResult};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::mpsc::{SyncSender, TrySendError};

pub const REPLAY_PATH_ENV: &str = "KAIRO_STT_REPLAY_PATH";
const QUEUE: usize = 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum RecordedEvent {
    Interim { result: InterimResult },
    Final { result: TranscriptResult },
    Gap { count: u64 },
}

pub struct TranscriptRecorder {
    tx: Option<SyncSender<RecordedEvent>>,
    writer: Option<std::thread::JoinHandle<()>>,
    dropped: u64,
}

/// `transcriptEvidenceKey`: where the speech starts in the provider's audio,
/// per connection.
pub fn transcript_evidence_key(
    audio_epoch: Option<f64>,
    audio_start: Option<f64>,
    fallback: &str,
) -> String {
    let epoch = audio_epoch.map_or("legacy".into(), kairo_domain::js::number_to_string);
    let start = audio_start.map_or(fallback.into(), kairo_domain::js::number_to_string);
    format!("{epoch}:{start}")
}

impl TranscriptRecorder {
    /// The recorder `KAIRO_STT_REPLAY_PATH` asks for, if any.
    pub fn from_env() -> Option<Self> {
        Self::create(&std::env::var(REPLAY_PATH_ENV).ok()?)
    }

    /// `None` for a blank path or a file that cannot be created (including
    /// one that already exists).
    pub fn create(path: &str) -> Option<Self> {
        if kairo_domain::js::trim(path).is_empty() {
            return None;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let file = match options.open(path) {
            Ok(file) => file,
            Err(error) => {
                tracing::warn!(%error, "transcript recording not started");
                return None;
            }
        };
        let (tx, rx) = std::sync::mpsc::sync_channel::<RecordedEvent>(QUEUE);
        let writer = std::thread::Builder::new()
            .name("kairo-transcript-recorder".into())
            .spawn(move || {
                let mut out = std::io::BufWriter::new(file);
                for event in rx {
                    let Ok(line) = serde_json::to_string(&event) else {
                        continue;
                    };
                    if writeln!(out, "{line}").is_err() {
                        return;
                    }
                }
                let _ = out.flush();
            })
            .ok()?;
        Some(Self {
            tx: Some(tx),
            writer: Some(writer),
            dropped: 0,
        })
    }

    pub fn record_interim(&mut self, result: &InterimResult) {
        self.record(RecordedEvent::Interim {
            result: result.clone(),
        });
    }

    pub fn record_final(&mut self, result: &TranscriptResult) {
        self.record(RecordedEvent::Final {
            result: result.clone(),
        });
    }

    fn record(&mut self, event: RecordedEvent) {
        let Some(tx) = &self.tx else { return };
        if self.dropped > 0 {
            match tx.try_send(RecordedEvent::Gap {
                count: self.dropped,
            }) {
                Ok(()) => self.dropped = 0,
                Err(TrySendError::Full(_)) => {
                    self.dropped += 1;
                    return;
                }
                Err(TrySendError::Disconnected(_)) => {
                    self.tx = None;
                    return;
                }
            }
        }
        match tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => self.dropped += 1,
            Err(TrySendError::Disconnected(_)) => self.tx = None,
        }
    }

    /// Flush and close. Marks a trailing gap.
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(tx) = self.tx.take() {
            if self.dropped > 0 {
                let _ = tx.send(RecordedEvent::Gap {
                    count: self.dropped,
                });
            }
            drop(tx);
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

impl Drop for TranscriptRecorder {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Read a recording back, refusing one with gaps.
pub fn read_recording(text: &str) -> Result<Vec<RecordedEvent>, String> {
    let mut events = Vec::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let event: RecordedEvent = serde_json::from_str(line).map_err(|e| e.to_string())?;
        if let RecordedEvent::Gap { count } = event {
            return Err(format!(
                "Incomplete transcript recording: {count} events dropped"
            ));
        }
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interim(text: &str) -> InterimResult {
        InterimResult {
            audio_epoch: Some(2.0),
            audio_start: Some(1.5),
            audio_duration: None,
            text: text.into(),
            stability: 0.9,
            timestamp: 1.0,
        }
    }

    #[test]
    fn records_and_reads_back_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        let path = path.to_str().unwrap();
        let mut recorder = TranscriptRecorder::create(path).unwrap();
        recorder.record_interim(&interim("Next verse"));
        recorder.close();
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.starts_with(r#"{"type":"interim","result":{"#));
        assert_eq!(
            read_recording(&text).unwrap(),
            [RecordedEvent::Interim {
                result: interim("Next verse")
            }]
        );
        assert!(
            TranscriptRecorder::create(path).is_none(),
            "never overwrites"
        );
        assert!(TranscriptRecorder::create("  ").is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn a_gap_is_refused() {
        let text = "{\"type\":\"gap\",\"count\":3}\n";
        assert_eq!(
            read_recording(text).unwrap_err(),
            "Incomplete transcript recording: 3 events dropped"
        );
    }

    #[test]
    fn evidence_keys_include_the_epoch() {
        assert_eq!(
            transcript_evidence_key(Some(12.0), Some(0.5), "x"),
            "12:0.5"
        );
        assert_eq!(transcript_evidence_key(None, None, "7"), "legacy:7");
    }
}
