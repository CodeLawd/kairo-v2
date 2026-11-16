//! Process inspection: resident memory, open descriptors, parents, names
//! and (macOS) the responsible process of XPC services such as the web
//! views' content processes. Only processes this soak launched, or that the
//! OS attributes to them, are ever signalled.

use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
}

/// Resident set size in bytes.
pub fn rss_bytes(pid: u32) -> Option<u64> {
    imp::rss_bytes(pid)
}

/// Open file descriptors (handles).
pub fn fd_count(pid: u32) -> Option<u64> {
    imp::fd_count(pid)
}

/// Every process the OS will describe to this user.
pub fn all() -> Vec<ProcInfo> {
    imp::all()
}

pub fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Sends `signal`; true when it was delivered.
pub fn signal(pid: u32, signal: i32) -> bool {
    // SAFETY: a plain syscall; callers pass pids this soak owns.
    unsafe { libc::kill(pid as i32, signal) == 0 }
}

/// Children (direct) of `parent` whose name contains `needle`.
pub fn children_named(parent: u32, needle: &str) -> Vec<ProcInfo> {
    all()
        .into_iter()
        .filter(|p| p.ppid == parent && p.name.contains(needle))
        .collect()
}

/// The web view content processes the OS attributes to `owner` (macOS
/// WebKit runs them as XPC services, parented to launchd; the OS records
/// the app responsible for them). Empty where the attribution call is
/// missing, so nothing else is ever touched.
pub fn web_content_of(owner: u32) -> Vec<ProcInfo> {
    all()
        .into_iter()
        .filter(|p| p.name.contains("WebContent") && imp::responsible(p.pid) == Some(owner))
        .collect()
}

/// The web content processes of this soak's engine. Only attributable when
/// this process is responsible for itself (see [`relaunch_disclaimed`]);
/// otherwise the terminal app is responsible and its other web views would
/// match too, so the answer is empty.
pub fn own_web_content() -> Vec<ProcInfo> {
    if can_attribute_web_content() {
        web_content_of(std::process::id())
    } else {
        Vec::new()
    }
}

/// Whether this soak's web content processes can be told apart.
pub fn can_attribute_web_content() -> bool {
    let me = std::process::id();
    imp::responsible(me) == Some(me)
}

/// Set in the relaunched soak.
pub const DISCLAIMED_ENV: &str = "KAIRO_SOAK_DISCLAIMED";

/// macOS: runs this program again as its own responsible process, so the
/// engine and its web views are attributed to it, and returns the exit
/// code. `None` when already relaunched, or on other systems, or when the
/// OS lacks the attribute (web content then counts as not verified).
pub fn relaunch_disclaimed() -> Option<i32> {
    if std::env::var_os(DISCLAIMED_ENV).is_some() {
        return None;
    }
    imp::relaunch_disclaimed()
}

/// Polls `check` every 20 ms until true or `within` passes.
pub async fn until(within: Duration, mut check: impl FnMut() -> bool) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    loop {
        if check() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ProcInfo;
    use std::ffi::CStr;
    use std::sync::OnceLock;

    pub fn rss_bytes(pid: u32) -> Option<u64> {
        // SAFETY: the buffer is a zeroed `proc_taskinfo` of the size passed.
        unsafe {
            let mut info: libc::proc_taskinfo = std::mem::zeroed();
            let size = std::mem::size_of::<libc::proc_taskinfo>() as i32;
            let got = libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTASKINFO,
                0,
                (&mut info as *mut libc::proc_taskinfo).cast(),
                size,
            );
            (got == size).then_some(info.pti_resident_size)
        }
    }

    pub fn fd_count(pid: u32) -> Option<u64> {
        // SAFETY: a null buffer asks for the size; the second call fills a
        // buffer of that many bytes.
        unsafe {
            let needed = libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDLISTFDS,
                0,
                std::ptr::null_mut(),
                0,
            );
            if needed <= 0 {
                return None;
            }
            let each = std::mem::size_of::<libc::proc_fdinfo>();
            let mut buffer: Vec<libc::proc_fdinfo> =
                vec![std::mem::zeroed(); needed as usize / each + 16];
            let got = libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDLISTFDS,
                0,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * each) as i32,
            );
            (got > 0).then(|| got as u64 / each as u64)
        }
    }

    pub fn all() -> Vec<ProcInfo> {
        // SAFETY: buffers are sized from the count the first call returns.
        unsafe {
            let count = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if count <= 0 {
                return Vec::new();
            }
            let mut pids = vec![0i32; count as usize + 64];
            let got = libc::proc_listallpids(
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            );
            pids.truncate(got.max(0) as usize);
            pids.into_iter()
                .filter(|pid| *pid > 0)
                .filter_map(|pid| {
                    let mut info: libc::proc_bsdinfo = std::mem::zeroed();
                    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
                    let got = libc::proc_pidinfo(
                        pid,
                        libc::PROC_PIDTBSDINFO,
                        0,
                        (&mut info as *mut libc::proc_bsdinfo).cast(),
                        size,
                    );
                    if got != size {
                        return None;
                    }
                    let mut name = [0u8; 256];
                    let len = libc::proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32);
                    let name = if len > 0 {
                        String::from_utf8_lossy(&name[..len as usize]).into_owned()
                    } else {
                        CStr::from_ptr(info.pbi_comm.as_ptr())
                            .to_string_lossy()
                            .into_owned()
                    };
                    Some(ProcInfo {
                        pid: pid as u32,
                        ppid: info.pbi_ppid,
                        name,
                    })
                })
                .collect()
        }
    }

    type Disclaim = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;

    pub fn relaunch_disclaimed() -> Option<i32> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: dlsym with RTLD_DEFAULT and a NUL-terminated name; the
        // private symbol has this signature where it exists.
        let disclaim = unsafe {
            let symbol = libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_spawnattrs_setdisclaim".as_ptr(),
            );
            if symbol.is_null() {
                return None;
            }
            std::mem::transmute::<*mut libc::c_void, Disclaim>(symbol)
        };
        let exe = std::env::current_exe().ok()?;
        let path = CString::new(exe.as_os_str().as_bytes()).ok()?;
        let args: Vec<CString> = std::env::args_os()
            .filter_map(|a| CString::new(a.as_bytes()).ok())
            .collect();
        let mut env: Vec<CString> = std::env::vars_os()
            .filter_map(|(k, v)| {
                let mut entry = k.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(v.as_bytes());
                CString::new(entry).ok()
            })
            .collect();
        env.push(CString::new(format!("{}=1", super::DISCLAIMED_ENV)).ok()?);
        let mut argv: Vec<*mut libc::c_char> = args.iter().map(|a| a.as_ptr().cast_mut()).collect();
        argv.push(std::ptr::null_mut());
        let mut envp: Vec<*mut libc::c_char> = env.iter().map(|e| e.as_ptr().cast_mut()).collect();
        envp.push(std::ptr::null_mut());
        // SAFETY: attr is initialised before use and destroyed after; argv
        // and envp are NUL-terminated arrays of live C strings.
        unsafe {
            let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
            if libc::posix_spawnattr_init(&mut attr) != 0 {
                return None;
            }
            if disclaim(&mut attr, 1) != 0 {
                libc::posix_spawnattr_destroy(&mut attr);
                return None;
            }
            let mut pid: libc::pid_t = 0;
            let spawned = libc::posix_spawn(
                &mut pid,
                path.as_ptr(),
                std::ptr::null(),
                &attr,
                argv.as_ptr(),
                envp.as_ptr(),
            );
            libc::posix_spawnattr_destroy(&mut attr);
            if spawned != 0 {
                return None;
            }
            // The terminal's Ctrl-C reaches the child too; outlive it.
            libc::signal(libc::SIGINT, libc::SIG_IGN);
            let mut status = 0;
            while libc::waitpid(pid, &mut status, 0) < 0 {
                if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                    return Some(70);
                }
            }
            Some(if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else {
                128 + libc::WTERMSIG(status)
            })
        }
    }

    type Responsible = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;

    /// `responsibility_get_pid_responsible_for_pid` from libSystem, looked
    /// up at run time: it is not in the public headers.
    pub fn responsible(pid: u32) -> Option<u32> {
        static FUNCTION: OnceLock<Option<Responsible>> = OnceLock::new();
        let function = FUNCTION.get_or_init(|| {
            // SAFETY: dlsym with RTLD_DEFAULT and a NUL-terminated name; the
            // symbol has this C signature on every macOS that exports it.
            unsafe {
                let symbol = libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"responsibility_get_pid_responsible_for_pid".as_ptr(),
                );
                (!symbol.is_null())
                    .then(|| std::mem::transmute::<*mut libc::c_void, Responsible>(symbol))
            }
        });
        let function = (*function)?;
        // SAFETY: the call takes any pid and returns -1 when unknown.
        let owner = unsafe { function(pid as libc::pid_t) };
        (owner > 0).then_some(owner as u32)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::ProcInfo;

    pub fn rss_bytes(pid: u32) -> Option<u64> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
        let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib * 1024)
    }

    pub fn fd_count(pid: u32) -> Option<u64> {
        Some(std::fs::read_dir(format!("/proc/{pid}/fd")).ok()?.count() as u64)
    }

    pub fn all() -> Vec<ProcInfo> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.filter_map(|entry| {
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let open = stat.find('(')?;
            let close = stat.rfind(')')?;
            let name = stat[open + 1..close].to_owned();
            let ppid = stat[close + 2..].split_whitespace().nth(1)?.parse().ok()?;
            Some(ProcInfo { pid, ppid, name })
        })
        .collect()
    }

    pub fn responsible(_pid: u32) -> Option<u32> {
        None
    }

    pub fn relaunch_disclaimed() -> Option<i32> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_described() {
        let me = std::process::id();
        assert!(rss_bytes(me).is_some_and(|b| b > 1024 * 1024));
        assert!(fd_count(me).is_some_and(|n| n >= 3));
        assert!(all().iter().any(|p| p.pid == me));
        assert!(alive(me));
    }

    #[test]
    fn missing_processes_are_none() {
        // Pid numbers are bounded well below this on every supported OS.
        let gone = 0x3fff_fff0;
        assert_eq!(rss_bytes(gone), None);
        assert_eq!(fd_count(gone), None);
        assert!(!alive(gone));
        assert!(web_content_of(gone).is_empty());
        // The test runner is not its own responsible process.
        if !can_attribute_web_content() {
            assert!(own_web_content().is_empty());
        }
    }

    #[test]
    fn children_are_found_by_parent_and_name() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let me = std::process::id();
        let found = children_named(me, "sleep");
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.iter().any(|p| p.pid == child.id()), "{found:?}");
    }
}
