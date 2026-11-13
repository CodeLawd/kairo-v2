//! RAII display-sleep inhibition, owned by the event-loop thread.
use std::time::{Duration, Instant};

pub struct Power {
    assertion: Option<Assertion>,
    retry: Instant,
    last_error: Option<String>,
}
impl Default for Power {
    fn default() -> Self {
        Self {
            assertion: None,
            retry: Instant::now(),
            last_error: None,
        }
    }
}
impl Power {
    pub fn active(&self) -> bool {
        self.assertion.is_some()
    }
    pub fn set_active(&mut self, active: bool) -> Option<String> {
        if !active {
            self.assertion = None;
            self.last_error = None;
            self.retry = Instant::now();
            return None;
        }
        if self.assertion.is_some() || Instant::now() < self.retry {
            return None;
        }
        self.retry = Instant::now() + Duration::from_secs(1);
        match Assertion::new() {
            Ok(assertion) => {
                self.assertion = Some(assertion);
                self.last_error = None;
                None
            }
            Err(error) => {
                let changed = self.last_error.as_ref() != Some(&error);
                self.last_error = Some(error.clone());
                changed.then_some(error)
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct Assertion(u32);
#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithName(
        kind: &objc2_core_foundation::CFString,
        level: u32,
        name: &objc2_core_foundation::CFString,
        id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(id: u32) -> i32;
}
#[cfg(target_os = "macos")]
impl Assertion {
    fn new() -> Result<Self, String> {
        use objc2_core_foundation::CFString;
        let mut id = 0;
        // SAFETY: retained CFStrings and writable assertion id; released on Drop.
        let result = unsafe {
            IOPMAssertionCreateWithName(
                &CFString::from_str("PreventUserIdleDisplaySleep"),
                255,
                &CFString::from_str("Kairo active display outputs"),
                &mut id,
            )
        };
        if result == 0 {
            Ok(Self(id))
        } else {
            Err(format!("Cannot prevent display sleep (IOKit {result})"))
        }
    }
}
#[cfg(target_os = "macos")]
impl Drop for Assertion {
    fn drop(&mut self) {
        unsafe {
            IOPMAssertionRelease(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
struct Assertion;
#[cfg(target_os = "windows")]
impl Assertion {
    fn new() -> Result<Self, String> {
        use windows::Win32::System::Power::*;
        if unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED) }.0 == 0 {
            Err("Cannot prevent display sleep".into())
        } else {
            Ok(Self)
        }
    }
}
#[cfg(target_os = "windows")]
impl Drop for Assertion {
    fn drop(&mut self) {
        use windows::Win32::System::Power::*;
        unsafe {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
struct Assertion;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Assertion {
    fn new() -> Result<Self, String> {
        Err("Display sleep inhibition is unavailable on this platform".into())
    }
}
