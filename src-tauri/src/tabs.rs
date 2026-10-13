//! Tab Manager (SRS §6.1).
//!
//! Creates, orders, renames and closes tabs.
//!
//! Must not reach into a tab's session or screen. It holds only handles, so it cannot.
//!
//! One per window. Its view, the tab bar, is `src/tabs.js`.
//!
//! Filled in by #31 (open, close, switch), #32 (closing), #33 (reordering) and #34 (titles).

/// The tabs of one window, in tab-bar order.
pub struct TabManager {
    #[expect(dead_code, reason = "read by #31")]
    tabs: Vec<tab::Handle>,
}

impl TabManager {
    /// A window's tabs before any is opened.
    pub fn new() -> Self {
        Self { tabs: Vec::new() }
    }

    /// The active tab's title, for the window's title bar (GEN-04). No tab has a title
    /// until #34.
    pub fn active_title(&self) -> Option<&str> {
        None
    }
}
