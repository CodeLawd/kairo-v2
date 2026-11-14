//! One Kairo per profile: the native counterpart of
//! `app.requestSingleInstanceLock()` and `second-instance` in `src/main/index.ts`.
//!
//! The first process takes an exclusive lock on a file in userData (where
//! Chromium keeps its own `SingletonLock`) and listens on a local socket. A
//! later process finds the lock held, sends its command line to the socket and
//! exits, so a `.kairo` file double-clicked on Windows or Linux opens in the
//! running app. The operating system drops the lock when a process dies, so a
//! crash never leaves Kairo unable to start.
//!
//! Dev builds skip the lock (`!is.dev` in Electron), so a dev build can run
//! beside the installed app; [`acquire`] is only called for packaged builds.

use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{ListenerOptions, Name};

/// The lock file in userData.
pub const LOCK_FILE_NAME: &str = "kairo-native.lock";

/// How long a second process keeps trying to reach the first, which may
/// still be starting its listener.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// What a second launch sends: its arguments and working folder, so
/// relative paths resolve where the user typed them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forwarded {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Forwarded {
    /// This process's arguments (without the executable) and folder.
    pub fn current() -> Self {
        Self {
            argv: std::env::args().skip(1).collect(),
            cwd: std::env::current_dir().ok(),
        }
    }

    /// The `.kairo` files named on the forwarded command line that exist.
    pub fn kairo_paths(&self) -> Vec<PathBuf> {
        crate::launch::kairo_paths_in(&self.argv, self.cwd.as_deref())
    }
}

/// The running instance. Dropping it releases the lock and stops listening.
pub struct PrimaryInstance {
    _lock: File,
    received: Receiver<Forwarded>,
    listener: Listening,
}

/// The listener thread and how to stop it.
struct Listening {
    name: String,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for PrimaryInstance {
    /// Closes the socket before the lock goes, so the next process (or a
    /// later [`acquire`] in this one) can bind the same name. The thread is
    /// blocked accepting; one connection wakes it to see `stop`.
    fn drop(&mut self) {
        let listening = &mut self.listener;
        listening.stop.store(true, Ordering::SeqCst);
        // The thread may still be answering the last launch and not yet
        // accepting again (on Windows there is then no pipe to connect to),
        // so the wake-up is retried for a moment.
        let deadline = Instant::now() + Duration::from_secs(2);
        let woken = loop {
            match to_name(&listening.name).and_then(LocalSocketStream::connect) {
                Ok(_) => break true,
                Err(_) if Instant::now() >= deadline => break false,
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        if let Some(thread) = listening.thread.take()
            && woken
        {
            let _ = thread.join();
        }
    }
}

impl PrimaryInstance {
    /// Launches forwarded by later processes, in arrival order. A message
    /// with no `.kairo` files means "bring the window forward".
    pub fn forwarded(&self) -> &Receiver<Forwarded> {
        &self.received
    }
}

/// The outcome of [`acquire`].
pub enum Instance {
    /// This process is the one Kairo.
    Primary(PrimaryInstance),
    /// Another process holds the lock and has received this launch. Exit.
    Secondary,
}

/// FNV-1a, for a short stable name derived from the userData folder.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The socket for a profile: a pipe name on Windows, the abstract namespace
/// on Linux, and a file in the per-user temporary folder on macOS.
pub fn socket_name(user_data: &Path) -> String {
    let id = format!("kairo-{:016x}", fnv1a(&user_data.to_string_lossy()));
    if cfg!(target_os = "macos") {
        std::env::temp_dir()
            .join(format!("{id}.sock"))
            .to_string_lossy()
            .into_owned()
    } else {
        format!("{id}.sock")
    }
}

fn to_name(name: &str) -> io::Result<Name<'_>> {
    if cfg!(target_os = "macos") {
        name.to_fs_name::<interprocess::local_socket::GenericFilePath>()
    } else {
        name.to_ns_name::<interprocess::local_socket::GenericNamespaced>()
    }
}

/// Takes the single-instance lock for `user_data`, or forwards `launch` to
/// the process that holds it.
pub fn acquire(user_data: &Path, launch: &Forwarded) -> io::Result<Instance> {
    std::fs::create_dir_all(user_data)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(user_data.join(LOCK_FILE_NAME))?;
    let name = socket_name(user_data);
    match lock.try_lock() {
        Ok(()) => {
            let (tx, rx) = mpsc::channel();
            let listener = listen(name, tx)?;
            Ok(Instance::Primary(PrimaryInstance {
                _lock: lock,
                received: rx,
                listener,
            }))
        }
        Err(TryLockError::WouldBlock) => {
            forward(&name, launch, CONNECT_TIMEOUT)?;
            Ok(Instance::Secondary)
        }
        Err(TryLockError::Error(error)) => Err(error),
    }
}

fn listen(name: String, tx: Sender<Forwarded>) -> io::Result<Listening> {
    // We hold the lock, so an existing socket file is stale.
    let listener = ListenerOptions::new()
        .name(to_name(&name)?)
        .try_overwrite(true)
        .create_sync()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::Builder::new()
        .name("kairo-instance".into())
        .spawn(move || {
            for connection in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                let Ok(stream) = connection else { continue };
                // A client that never sends a line must not hold up later
                // launches. Timeouts are unsupported on Windows named pipes.
                let _ = stream.set_recv_timeout(Some(CONNECT_TIMEOUT));
                let _ = stream.set_send_timeout(Some(CONNECT_TIMEOUT));
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                match serde_json::from_str::<Forwarded>(line.trim_end()) {
                    Ok(message) => {
                        let _ = reader.get_mut().write_all(b"ok\n");
                        if tx.send(message).is_err() {
                            return; // The app has gone.
                        }
                    }
                    Err(error) => tracing::warn!(%error, "[Instance] Ignored a malformed message"),
                }
            }
        })?;
    Ok(Listening {
        name,
        stop,
        thread: Some(thread),
    })
}

/// Sends `launch` to the first instance and waits for it to confirm.
pub fn forward(name: &str, launch: &Forwarded, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let stream = loop {
        match LocalSocketStream::connect(to_name(name)?) {
            Ok(stream) => break stream,
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_secs(1));
    let _ = stream.set_recv_timeout(Some(remaining));
    let _ = stream.set_send_timeout(Some(remaining));
    let mut reader = BufReader::new(stream);
    let mut message = serde_json::to_string(launch).map_err(io::Error::other)?;
    message.push('\n');
    reader.get_mut().write_all(message.as_bytes())?;
    let mut reply = String::new();
    reader.read_line(&mut reply)?;
    if reply.trim_end() == "ok" {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the running Kairo did not confirm",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_names_differ_per_profile_and_are_stable() {
        let a = socket_name(Path::new("/Users/ada/Library/Application Support/Kairo"));
        let b = socket_name(Path::new(
            "/Users/ada/Library/Application Support/Kairo Dev",
        ));
        assert_ne!(a, b);
        assert_eq!(
            a,
            socket_name(Path::new("/Users/ada/Library/Application Support/Kairo"))
        );
        // Unix socket paths are limited to about 104 bytes.
        assert!(a.len() < 104, "{a}");
    }

    #[test]
    fn a_second_acquire_in_one_process_forwards_to_the_first() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Song.KAIRO");
        std::fs::write(&file, "x").unwrap();
        let Instance::Primary(primary) = acquire(
            dir.path(),
            &Forwarded {
                argv: vec![],
                cwd: None,
            },
        )
        .unwrap() else {
            panic!("the first acquire must win");
        };
        let launch = Forwarded {
            argv: vec!["--flag".into(), "Song.KAIRO".into()],
            cwd: Some(dir.path().into()),
        };
        assert!(matches!(
            acquire(dir.path(), &launch).unwrap(),
            Instance::Secondary
        ));
        let got = primary
            .forwarded()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(got, launch);
        assert_eq!(got.kairo_paths(), vec![file]);
        // Dropping the primary frees the lock for the next launch.
        drop(primary);
        assert!(matches!(
            acquire(dir.path(), &launch).unwrap(),
            Instance::Primary(_)
        ));
    }

    #[test]
    fn forwarding_to_nobody_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let name = socket_name(dir.path());
        let error = forward(
            &name,
            &Forwarded {
                argv: vec![],
                cwd: None,
            },
            Duration::from_millis(200),
        );
        assert!(error.is_err());
    }
}
