// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! Memory tracking: a global allocator that counts.
//!
//! Install it once, in the binary (a library cannot choose the allocator):
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: kerosene::lifecycle::TrackingAllocator = kerosene::lifecycle::TrackingAllocator::new();
//! ```
//!
//! [`snapshot`] then reports live and peak bytes and the allocation count,
//! overall and per *tag*. A tag is a name -- a module's, for the threads the
//! manager starts -- and every allocation is counted under the tag of the
//! thread that made it. A free is counted under the tag of the thread that
//! frees, so a buffer handed from one thread to another moves with it; a
//! tag's live figure can therefore dip below zero, and is shown signed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering};

/// How many distinct tags there can be, `misc` included.
const MAX_TAGS: usize = 32;

/// A name allocations are counted under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag(u8);

impl Tag {
    /// Allocations nothing else claims.
    pub const MISC: Tag = Tag(0);
}

static INSTALLED: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static FREES: AtomicUsize = AtomicUsize::new(0);
static TAG_LIVE: [AtomicIsize; MAX_TAGS] = [const { AtomicIsize::new(0) }; MAX_TAGS];
static TAG_ALLOCS: [AtomicUsize; MAX_TAGS] = [const { AtomicUsize::new(0) }; MAX_TAGS];
static NAMES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

thread_local! {
    // A `Cell<u8>` with a constant start: no allocation and no destructor, so
    // the allocator can read it without recursing.
    static CURRENT: Cell<u8> = const { Cell::new(0) };
}

/// The tag for `name`, made on first use. Past the limit, new names share
/// `misc`.
pub fn tag(name: &'static str) -> Tag {
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = names.iter().position(|n| *n == name) {
        return Tag(i as u8 + 1);
    }
    if names.len() + 1 >= MAX_TAGS {
        return Tag::MISC;
    }
    names.push(name);
    Tag(names.len() as u8)
}

/// Count this thread's allocations under `tag` from now on.
pub fn set_thread_tag(tag: Tag) {
    let _ = CURRENT.try_with(|c| c.set(tag.0));
}

/// Counts this thread's allocations under a tag until dropped, then
/// restores the tag it had.
#[must_use = "the tag is only held while the scope is alive"]
pub struct Scope {
    previous: u8,
}

/// Count this thread's allocations under `tag` for as long as the returned
/// guard lives. For main-thread phases that have no thread of their own.
pub fn scope(tag: Tag) -> Scope {
    let previous = CURRENT.try_with(|c| c.replace(tag.0)).unwrap_or(0);
    Scope { previous }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let _ = CURRENT.try_with(|c| c.set(self.previous));
    }
}

fn current() -> usize {
    CURRENT.try_with(Cell::get).unwrap_or(0) as usize
}

fn record_alloc(size: usize) {
    INSTALLED.store(true, Ordering::Relaxed);
    let live = LIVE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(live, Ordering::Relaxed);
    ALLOCS.fetch_add(1, Ordering::Relaxed);
    let tag = current();
    TAG_LIVE[tag].fetch_add(size as isize, Ordering::Relaxed);
    TAG_ALLOCS[tag].fetch_add(1, Ordering::Relaxed);
}

fn record_free(size: usize) {
    LIVE.fetch_sub(size, Ordering::Relaxed);
    FREES.fetch_add(1, Ordering::Relaxed);
    TAG_LIVE[current()].fetch_sub(size as isize, Ordering::Relaxed);
}

/// A global allocator that forwards to another (the system's, by default)
/// and counts what passes through. See the [module docs](self).
pub struct TrackingAllocator<A = System>(A);

impl TrackingAllocator<System> {
    /// Track the system allocator.
    pub const fn new() -> Self {
        Self(System)
    }
}

impl Default for TrackingAllocator<System> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A> TrackingAllocator<A> {
    /// Track `inner`, which may be any allocator.
    pub const fn wrapping(inner: A) -> Self {
        Self(inner)
    }
}

// SAFETY: every method forwards to the wrapped allocator with the arguments
// it was given, and only reads or writes counters besides. The counters never
// allocate (atomics and a constant-initialised thread local), so the
// allocator cannot re-enter itself.
unsafe impl<A: GlobalAlloc> GlobalAlloc for TrackingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for
        // `layout`, which is passed on unchanged.
        let ptr = unsafe { self.0.alloc(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let ptr = unsafe { self.0.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` are the pair the caller got from this
        // allocator, which handed them out by forwarding to `self.0`.
        unsafe { self.0.dealloc(ptr, layout) };
        record_free(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as for `dealloc`, with `new_size` checked by the caller.
        let new = unsafe { self.0.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            record_free(layout.size());
            record_alloc(new_size);
        }
        new
    }
}

/// One tag's share of the counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagStats {
    /// The tag's name; `misc` for everything untagged.
    pub name: &'static str,
    /// Bytes allocated minus bytes freed under this tag. Signed; see the
    /// [module docs](self).
    pub live: isize,
    /// Allocations made under this tag.
    pub allocs: usize,
}

/// The counts at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemStats {
    /// Whether a [`TrackingAllocator`] is installed and counting. If not,
    /// every figure here is zero.
    pub installed: bool,
    /// Bytes allocated and not yet freed.
    pub live: usize,
    /// The most `live` has been since the start, or since [`reset_peak`].
    pub peak: usize,
    /// Allocations made.
    pub allocs: usize,
    /// Allocations freed.
    pub frees: usize,
    /// Per tag, those that have seen any allocation, `misc` first.
    pub tags: Vec<TagStats>,
}

/// Read the counters.
pub fn snapshot() -> MemStats {
    let names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let mut tags = Vec::new();
    for i in 0..MAX_TAGS {
        let allocs = TAG_ALLOCS[i].load(Ordering::Relaxed);
        if allocs == 0 && i != 0 {
            continue;
        }
        let name = if i == 0 {
            "misc"
        } else {
            names.get(i - 1).copied().unwrap_or("?")
        };
        tags.push(TagStats {
            name,
            live: TAG_LIVE[i].load(Ordering::Relaxed),
            allocs,
        });
    }
    MemStats {
        installed: INSTALLED.load(Ordering::Relaxed),
        live: LIVE.load(Ordering::Relaxed),
        peak: PEAK.load(Ordering::Relaxed),
        allocs: ALLOCS.load(Ordering::Relaxed),
        frees: FREES.load(Ordering::Relaxed),
        tags,
    }
}

/// Start the peak over from what is live now.
pub fn reset_peak() {
    PEAK.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// `bytes` as a person reads it: `1.5 MiB`.
pub fn format_bytes(bytes: i128) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let sign = if bytes < 0 { "-" } else { "" };
    let mut value = bytes.unsigned_abs() as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{sign}{} B", value as u64)
    } else {
        format!("{sign}{value:.1} {}", UNITS[unit])
    }
}

impl MemStats {
    /// The lines the `mem` console command prints.
    pub fn report(&self) -> Vec<String> {
        if !self.installed {
            return vec![
                "memory tracking is not installed: add a `#[global_allocator]` of \
                 `kerosene::lifecycle::TrackingAllocator` to the binary"
                    .to_string(),
            ];
        }
        let mut lines = vec![format!(
            "live {}  peak {}  allocations {}  freed {}",
            format_bytes(self.live as i128),
            format_bytes(self.peak as i128),
            self.allocs,
            self.frees
        )];
        for tag in &self.tags {
            lines.push(format!(
                "  {:<12} {:>10}  {} allocations",
                tag.name,
                format_bytes(tag.live as i128),
                tag.allocs
            ));
        }
        lines
    }
}
