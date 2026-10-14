//! Tab Manager (SRS §6.1).
//!
//! Creates, orders, renames and closes tabs.
//!
//! Must not reach into a tab's session or screen. It holds only handles, so it cannot.
//!
//! One per window. Its view, the tab bar, is `src/tabs.js`.
//!
//! Filled in by #31 (open, close, switch), #32 (closing), #33 (reordering) and #34 (titles).

use std::path::PathBuf;

/// The tabs of one window, in tab-bar order.
pub struct TabManager {
    #[expect(dead_code, reason = "read by #31")]
    tabs: Vec<tab::Handle>,
    /// Where the first tab starts. `None` is the home directory (FISH-01).
    #[expect(dead_code, reason = "read by #31")]
    directory: Option<PathBuf>,
}

impl TabManager {
    /// A window's tabs before any is opened. The first one will start in `directory`: the one
    /// given to the launcher, or with none the home directory (FISH-01).
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self {
            tabs: Vec::new(),
            directory,
        }
    }

    /// The active tab's title, for the window's title bar (GEN-04). No tab has a title
    /// until #34.
    pub fn active_title(&self) -> Option<&str> {
        None
    }
}
