//! Renderer (SRS §6.1).
//!
//! Draws the Terminal Core's state and the command line with the active font, ligatures and
//! palette.
//!
//! Must not change terminal state. This half only reads: it turns what changed in the
//! Terminal Core and the Command Line into a [`Frame`]. Its other half,
//! `src/tab/renderer.js`, draws frames on the window's canvas.
//!
//! A tab sends a new frame only after the last one was drawn, so frames never queue behind
//! heavy output (PERF-07).
//!
//! Filled in by #22 (grid), #23 (fonts), #24 (ligatures) and #25 (cursor).

/// One tab's render state: what the window was last sent and what has changed since.
pub struct Renderer;

/// What to draw: the rows that changed, the cursor, the selection and whatever overlays the
/// terminal area. The wire format between the two halves of the Renderer. No other code
/// reads it.
pub struct Frame;
