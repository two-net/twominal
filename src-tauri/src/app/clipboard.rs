//! The OS clipboard and, on Linux, the primary selection (SW-05), through arboard: the
//! `tab::OsClipboard` the App Shell gives each tab with its Host (#31). The user's copies and
//! pastes (TERM-17) and what programs ask of the clipboard (OSC 52, SEC-04) both come here.
//!
//! One arboard clipboard is kept for as long as Twominal runs, made at the first call, and at the
//! next call again for as long as none can be made, as while X11 or the compositor is not up yet.
//! On Linux the program that put
//! text on the clipboard or the primary selection serves it to the others, under X11 and under
//! Wayland, and arboard serves it from its own thread for as long as a clipboard of its lives. On
//! Windows arboard opens the OS's clipboard afresh for each call, since only one program may have
//! it open at a time.
//!
//! The primary selection is Linux's. Elsewhere a write to it is dropped and a read finds
//! nothing, as DESIGN.md §3 says.

use std::sync::{Mutex, MutexGuard, PoisonError};

#[cfg(target_os = "linux")]
use arboard::{GetExtLinux, LinuxClipboardKind, SetExtLinux};
use tab::{OsClipboard, Selection};

/// The OS clipboard. Without one, as on Linux with neither X11 nor a Wayland compositor that
/// lets programs reach it, nothing is written and nothing is read.
pub(super) struct Clipboard {
    os: Mutex<Option<arboard::Clipboard>>,
}

// The ignored test of the App Shell's uses it already.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "#31 gives it to each tab with its Host")
)]
impl Clipboard {
    pub(super) fn new() -> Self {
        Self {
            os: Mutex::new(None),
        }
    }

    /// The OS clipboard, for one call at a time: arboard may fail a second call made while
    /// another is under way. None while there is none to reach.
    fn os(&self) -> MutexGuard<'_, Option<arboard::Clipboard>> {
        let mut os = self.os.lock().unwrap_or_else(PoisonError::into_inner);
        if os.is_none() {
            *os = arboard::Clipboard::new().ok();
        }
        os
    }
}

impl OsClipboard for Clipboard {
    fn write(&self, which: Selection, text: &str) {
        let mut os = self.os();
        let Some(os) = os.as_mut() else {
            return;
        };
        // A write that fails leaves the clipboard as it was, and nothing more can be done.
        let _ = match which {
            Selection::Clipboard => os.set_text(text),
            #[cfg(target_os = "linux")]
            Selection::Primary => os.set().clipboard(LinuxClipboardKind::Primary).text(text),
            #[cfg(not(target_os = "linux"))]
            Selection::Primary => return,
        };
    }

    fn read(&self, which: Selection) -> Option<String> {
        let mut os = self.os();
        let os = os.as_mut()?;
        match which {
            Selection::Clipboard => os.get_text().ok(),
            #[cfg(target_os = "linux")]
            Selection::Primary => os.get().clipboard(LinuxClipboardKind::Primary).text().ok(),
            #[cfg(not(target_os = "linux"))]
            Selection::Primary => None,
        }
    }
}
