//! The hyperlinks programs make of the text they print (OSC 8, TERM-13).
//!
//! A cell holds the number of its link, and the link itself is kept here, once. Links with an
//! `id` are the same link wherever the program draws them again, as the OSC 8 specification
//! asks, so a full-screen program that redraws one keeps one entry. Without an `id` each OSC 8
//! makes a link of its own.
//!
//! The table is bounded (REL-02, PERF-10). When it is full, the links no cell holds any more
//! are freed by a sweep over the screens and the scrollback, as kitty frees its own. A link
//! that still does not fit is not made, and its text prints plain. No sweep runs until links
//! that take as much as the last sweep kept, counted as the table counts them, have been asked
//! for since, so a program has to send as much again for each walk over the scrollback.

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::num::NonZeroU16;

use crate::screen::Cell;

/// The most links the table keeps.
#[cfg(not(any(test, fuzzing)))]
const MOST: usize = u16::MAX as usize;
/// Few, so that the fuzz targets and the tests reach the sweep.
#[cfg(any(test, fuzzing))]
pub(crate) const MOST: usize = 8;

/// The most memory the links may take, counted as their text and [`OVERHEAD`] each.
#[cfg(not(any(test, fuzzing)))]
const BYTES: usize = 2 * 1024 * 1024;
/// Room for a few of the longest links.
#[cfg(any(test, fuzzing))]
pub(crate) const BYTES: usize = 3 * (URI_MOST + ID_MOST + OVERHEAD);

/// What a link is counted to take besides its text: its slot, its entry among the named links
/// and the allocations' own.
pub(crate) const OVERHEAD: usize = 64;

/// The longest URI a link may have, in bytes: VTE's, which is the longest Internet Explorer
/// opened.
pub(crate) const URI_MOST: usize = 2083;

/// The longest `id` a link may have, in bytes: VTE's.
pub(crate) const ID_MOST: usize = 250;

/// Names a link for as long as a cell holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LinkId(NonZeroU16);

impl LinkId {
    fn slot(self) -> usize {
        usize::from(self.0.get()) - 1
    }

    fn of_slot(slot: usize) -> Self {
        // A slot is below `MOST`, so it fits in a u16 with 1 added.
        Self(NonZeroU16::new(slot as u16 + 1).expect("a slot plus one is not zero"))
    }
}

/// A hyperlink a program made (OSC 8): where it leads, and the `id` it gave, if any. Opening it
/// is the user's to do, and only for the schemes SEC-07 allows (#30).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    uri: Arc<str>,
    id: Option<Arc<str>>,
}

impl Link {
    /// The target: bytes from space to `~`, at most 2083 of them.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// The `id` the program gave the link. Cells with the same `id` and URI are one link,
    /// wherever they are.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    fn bytes(&self) -> usize {
        self.uri.len() + self.id.as_ref().map_or(0, |id| id.len()) + OVERHEAD
    }
}

/// The table is full.
#[derive(Debug)]
struct Full;

/// Every link a cell may hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Links {
    /// The links by their number, less one. `None` where one was freed.
    slots: Vec<Option<Link>>,
    /// The slots freed, the lowest last, to be used again from the lowest.
    free: Vec<usize>,
    /// The links with an `id`, by their `id` and then their URI.
    named: BTreeMap<Arc<str>, BTreeMap<Arc<str>, LinkId>>,
    /// What the links take, as [`Link::bytes`] counts it.
    bytes: usize,
    /// What the links asked for since the last sweep take, counted the same way, whether they
    /// were made or not.
    asked: usize,
    /// What the links the last sweep left take. No sweep runs until as much has been asked for.
    kept: usize,
}

impl Links {
    pub(crate) fn get(&self, link: LinkId) -> Option<&Link> {
        self.slots.get(link.slot())?.as_ref()
    }

    /// The link to `uri` with `id`: the one made for both before, when there is an `id`, or a
    /// new one. `uri` and `id` must be no longer than [`URI_MOST`] and [`ID_MOST`]. When the
    /// table is full, the links none of `cells` holds are freed first, if a sweep is due. None
    /// if it is still full.
    pub(crate) fn make<'a, Cells>(
        &mut self,
        uri: &str,
        id: Option<&str>,
        cells: impl FnOnce() -> Cells,
    ) -> Option<LinkId>
    where
        Cells: Iterator<Item = &'a Cell>,
    {
        let bytes = uri.len() + id.map_or(0, str::len) + OVERHEAD;
        self.asked = self.asked.saturating_add(bytes);
        match self.add(uri, id) {
            Ok(made) => Some(made),
            // Enough was asked for since the last sweep for another to be worth it.
            Err(Full) if self.asked >= self.kept => {
                self.sweep(cells());
                self.add(uri, id).ok()
            }
            Err(Full) => None,
        }
    }

    /// The link to `uri` with `id`, as [`Links::make`] gives it, without a sweep.
    fn add(&mut self, uri: &str, id: Option<&str>) -> Result<LinkId, Full> {
        if let Some(made) = id.and_then(|id| self.named.get(id)?.get(uri)) {
            return Ok(*made);
        }
        let link = Link {
            uri: Arc::from(uri),
            id: id.map(Arc::from),
        };
        let bytes = link.bytes();
        if self.slots.len() - self.free.len() == MOST || self.bytes + bytes > BYTES {
            return Err(Full);
        }
        let slot = self.free.pop().unwrap_or_else(|| {
            self.slots.push(None);
            self.slots.len() - 1
        });
        let made = LinkId::of_slot(slot);
        if let Some(id) = &link.id {
            self.named
                .entry(id.clone())
                .or_default()
                .insert(link.uri.clone(), made);
        }
        self.bytes += bytes;
        self.slots[slot] = Some(link);
        Ok(made)
    }

    /// Frees every link none of `cells` holds. A link that is kept keeps its number, so no
    /// cell changes.
    fn sweep<'a>(&mut self, cells: impl IntoIterator<Item = &'a Cell>) {
        let mut held = vec![false; self.slots.len()];
        for link in cells.into_iter().filter_map(Cell::link) {
            held[link.slot()] = true;
        }
        for (slot, held) in held.into_iter().enumerate() {
            if held {
                continue;
            }
            let Some(freed) = self.slots[slot].take() else {
                continue;
            };
            if let Some(id) = &freed.id {
                if let Some(uris) = self.named.get_mut(id) {
                    uris.remove(&freed.uri);
                    if uris.is_empty() {
                        self.named.remove(id);
                    }
                }
            }
            self.bytes -= freed.bytes();
            self.free.push(slot);
        }
        self.free.sort_unstable_by(|a, b| b.cmp(a));
        self.kept = self.bytes;
        self.asked = 0;
    }

    /// Panics if the table breaks its bounds, or its counts do not add up.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn check(&self) {
        let kept: Vec<&Link> = self.slots.iter().flatten().collect();
        assert_eq!(kept.len() + self.free.len(), self.slots.len(), "{self:?}");
        assert!(kept.len() <= MOST, "{self:?}");
        assert_eq!(
            self.bytes,
            kept.iter().map(|link| link.bytes()).sum::<usize>(),
            "{self:?}"
        );
        assert!(self.bytes <= BYTES, "{self:?}");
        let named = self.named.values().map(BTreeMap::len).sum::<usize>();
        assert_eq!(
            named,
            kept.iter().filter(|link| link.id.is_some()).count(),
            "{self:?}"
        );
        for (id, uris) in &self.named {
            assert!(!uris.is_empty(), "{self:?}");
            for (uri, &link) in uris {
                let held = self.get(link).expect("a named link is kept");
                assert!(held.id.as_ref() == Some(id) && held.uri == *uri, "{self:?}");
            }
        }
        for &slot in &self.free {
            assert!(self.slots[slot].is_none(), "{self:?}");
        }
    }
}
