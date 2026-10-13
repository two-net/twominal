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
#[expect(dead_code, reason = "constructed by #31")]
pub struct TabManager {
    tabs: Vec<tab::Handle>,
}
