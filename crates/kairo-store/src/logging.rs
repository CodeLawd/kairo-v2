//! Logging through `tracing`, to the folder electron-log uses.
//!
//! As in `src/main/index.ts`: `info` and above go to the file; the console
//! shows `warn` and above in a packaged build and `debug` in a dev build.
//! The file is written on a background thread through a non-blocking
//! channel, so a log call never waits on the disk. Like electron-log, the
//! file rotates at 1 MiB to `<name>.old.log`, keeping one old file.
//!
//! Lines use electron-log's layout, `[2026-11-09 10:00:00.123] [info]  text`,
//! so both apps' logs read the same.
//!
//! Secrets: nothing in this module inspects values. Callers never log
//! settings or credentials; settings go through
//! `kairo_domain::cloud::org_secrets::redact_settings_secrets` first, and
//! crash text goes through `kairo_domain::cloud::scrub_home`.

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{Layer, filter::LevelFilter};

/// electron-log's `maxSize`: 1 MiB.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;

/// A file that moves itself to `<stem>.old.<ext>` once it would pass `max_bytes`.
pub struct RotatingFile {
    path: PathBuf,
    old: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
}

impl RotatingFile {
    /// Opens `path` for appending, creating its folder. A file already over
    /// the limit rotates on the first write.
    pub fn open(path: &Path, max_bytes: u64) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("log");
        let old = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => path.with_file_name(format!("{stem}.old.{ext}")),
            None => path.with_file_name(format!("{stem}.old")),
        };
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata()?.len();
        Ok(Self {
            path: path.into(),
            old,
            max_bytes,
            file: Some(file),
            size,
        })
    }

    /// The rotated file's path.
    pub fn old_path(&self) -> &Path {
        &self.old
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        // electron-log replaces the old file; a failed rename keeps writing
        // to the current file rather than losing lines.
        if let Err(error) = std::fs::rename(&self.path, &self.old) {
            let _ = std::fs::remove_file(&self.old);
            std::fs::rename(&self.path, &self.old).map_err(|_| error)?;
        }
        self.file = Some(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
        );
        self.size = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.size > 0 && self.size + buf.len() as u64 > self.max_bytes && self.rotate().is_err()
        {
            // Reopen the original if rotation left no file open.
            if self.file.is_none() {
                self.file = Some(
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&self.path)?,
                );
            }
        }
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file closed"))?;
        let written = file.write(buf)?;
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.as_mut().map_or(Ok(()), File::flush)
    }
}

/// Formats events as electron-log does: `[time] [level] message key=value`.
pub struct ElectronLogFormat;

struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.rest, " {}={value:?}", field.name());
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.rest, " {}={value:?}", field.name());
        }
    }
}

/// electron-log's level names.
pub fn level_name(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "silly",
    }
}

/// One line in electron-log's layout, without the trailing newline.
pub fn format_line(time: &chrono::DateTime<chrono::Local>, level: &Level, text: &str) -> String {
    format!(
        "[{}] {:<7} {text}",
        time.format("%Y-%m-%d %H:%M:%S%.3f"),
        format!("[{}]", level_name(level))
    )
}

impl<S, N> FormatEvent<S, N> for ElectronLogFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let mut fields = Fields {
            message: String::new(),
            rest: String::new(),
        };
        event.record(&mut fields);
        let text = format!("{}{}", fields.message, fields.rest);
        writeln!(
            writer,
            "{}",
            format_line(&chrono::Local::now(), event.metadata().level(), &text)
        )
    }
}

/// Keeps the background writer alive. Dropping it flushes what is queued.
#[must_use = "dropping the guard stops file logging"]
pub struct LogGuard {
    _worker: WorkerGuard,
}

/// The console level: `warn` for a packaged build, `debug` for a dev build.
pub fn console_level(packaged: bool) -> LevelFilter {
    if packaged {
        LevelFilter::WARN
    } else {
        LevelFilter::DEBUG
    }
}

/// The non-blocking file writer and its guard, for `log_file`.
pub fn file_writer(log_file: &Path) -> io::Result<(NonBlocking, WorkerGuard)> {
    let file = RotatingFile::open(log_file, MAX_LOG_BYTES)?;
    Ok(tracing_appender::non_blocking(file))
}

/// Installs the global subscriber: `info` to `log_file`, and the console at
/// [`console_level`]. Fails if the file cannot be opened or a subscriber is
/// already installed; the caller then runs with console logging only.
pub fn init(log_file: &Path, packaged: bool) -> io::Result<LogGuard> {
    let (writer, guard) = file_writer(log_file)?;
    let file = tracing_subscriber::fmt::layer()
        .event_format(ElectronLogFormat)
        .with_writer(writer)
        .with_ansi(false)
        .with_filter(LevelFilter::INFO);
    let console = tracing_subscriber::fmt::layer()
        .event_format(ElectronLogFormat)
        .with_writer(io::stderr)
        .with_filter(console_level(packaged));
    let subscriber = tracing_subscriber::registry().with(file).with(console);
    tracing::subscriber::set_global_default(subscriber).map_err(io::Error::other)?;
    Ok(LogGuard { _worker: guard })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_match_electron_log() {
        let time = chrono::TimeZone::with_ymd_and_hms(&chrono::Local, 2026, 11, 7, 12, 57, 26)
            .unwrap()
            + chrono::Duration::milliseconds(694);
        assert_eq!(
            format_line(&time, &Level::WARN, "x"),
            "[2026-11-07 12:57:26.694] [warn]  x"
        );
        assert_eq!(
            format_line(&time, &Level::INFO, "x"),
            "[2026-11-07 12:57:26.694] [info]  x"
        );
        assert_eq!(
            format_line(&time, &Level::ERROR, "x"),
            "[2026-11-07 12:57:26.694] [error] x"
        );
        assert_eq!(
            format_line(&time, &Level::DEBUG, "x"),
            "[2026-11-07 12:57:26.694] [debug] x"
        );
    }

    #[test]
    fn rotates_at_the_limit_keeping_one_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("kairo.log");
        let mut file = RotatingFile::open(&path, 10).unwrap();
        assert_eq!(file.old_path(), dir.path().join("logs/kairo.old.log"));
        file.write_all(b"12345678\n").unwrap();
        file.write_all(b"abc\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abc\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("logs/kairo.old.log")).unwrap(),
            "12345678\n"
        );
        file.write_all(b"defghij\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("logs/kairo.old.log")).unwrap(),
            "abc\n"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "defghij\n");
    }

    #[test]
    fn a_line_longer_than_the_limit_is_still_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kairo.log");
        let mut file = RotatingFile::open(&path, 4).unwrap();
        file.write_all(b"longer than four\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "longer than four\n"
        );
    }

    #[test]
    fn an_existing_large_file_rotates_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kairo.log");
        std::fs::write(&path, "0123456789").unwrap();
        let mut file = RotatingFile::open(&path, 10).unwrap();
        file.write_all(b"new\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    }

    #[test]
    fn console_levels_follow_the_build() {
        assert_eq!(console_level(true), LevelFilter::WARN);
        assert_eq!(console_level(false), LevelFilter::DEBUG);
    }

    #[test]
    fn events_reach_the_file_through_the_background_writer() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kairo.log");
        let (writer, guard) = file_writer(&path).unwrap();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ElectronLogFormat)
                .with_writer(writer)
                .with_filter(LevelFilter::INFO),
        );
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(count = 2, "[Store] hello");
            tracing::debug!("not written");
        });
        drop(guard);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.ends_with("] [info]  [Store] hello count=2\n"),
            "{text}"
        );
        assert!(!text.contains("not written"));
    }
}
