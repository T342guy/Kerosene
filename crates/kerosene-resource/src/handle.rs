// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0
//! Handles to loaded resources, and the cache that hands them out.
//!
//! Code that wants a material asks [`Resources`] for it by path and keeps a
//! [`Resource<Material>`]. The handle, not the material, is what it holds:
//!
//! - **One copy.** Every request for the same path and type gets the same
//!   handle, so a material read by the renderer and by the footstep code is
//!   read and decoded once.
//! - **Loading later.** [`Resources::request`] returns at once with a handle
//!   in [`LoadState::Queued`]; [`Resources::pump`] does the work when the
//!   caller has time for it, and the handle becomes ready.
//! - **Hot reload.** [`Resources::reload`] swaps the value behind every handle
//!   to a path and bumps its [`generation`](Resource::generation), so
//!   whatever built GPU data from it knows to build it again. Listeners
//!   added with [`Resources::on_reload`] hear the path.
//! - **Freeing.** [`Resources::collect`] drops what nothing holds a handle to.
//!
//! The cache does not own the file system: each call that reads takes a
//! [`Source`], so the engine keeps mounting and unmounting its [`Vfs`] as it
//! always has, and a test can hand in a map of bytes.
//!
//! [`Vfs`]: kerosene_vfs::Vfs

use crate::{ResourceError, ResourceType, decode};
use std::any::{Any, TypeId};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, RwLock};

/// Where [`Resources`] reads bytes from.
pub trait Source {
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
}

impl Source for kerosene_vfs::Vfs {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        kerosene_vfs::Vfs::read(self, path).map_err(|e| e.to_string())
    }
}

/// Paths to bytes, for tests and for tools that already have the files.
impl Source for HashMap<String, Vec<u8>> {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.get(path)
            .cloned()
            .ok_or_else(|| format!("{path:?} was not found"))
    }
}

/// Where a handle's load has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Asked for, not yet read.
    Queued,
    /// Loaded; [`Resource::get`] has it.
    Ready,
    /// Would not read or decode. The message says why.
    Failed(String),
}

struct Slot<T> {
    path: String,
    inner: RwLock<SlotInner<T>>,
}

struct SlotInner<T> {
    state: LoadState,
    value: Option<Arc<T>>,
    generation: u64,
}

/// A shared handle to one resource.
///
/// Cloning it is cheap and every clone sees the same value, including after
/// a reload.
pub struct Resource<T> {
    slot: Arc<Slot<T>>,
}

impl<T> Clone for Resource<T> {
    fn clone(&self) -> Self {
        Resource {
            slot: Arc::clone(&self.slot),
        }
    }
}

impl<T> std::fmt::Debug for Resource<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resource")
            .field("path", &self.slot.path)
            .field("state", &self.state())
            .finish()
    }
}

impl<T> Resource<T> {
    /// The virtual path it was asked for by.
    pub fn path(&self) -> &str {
        &self.slot.path
    }

    /// The value, once it has loaded.
    ///
    /// A clone of an `Arc`, so it stays valid if the resource is reloaded
    /// while it is held: the holder keeps the old value until it asks again.
    pub fn get(&self) -> Option<Arc<T>> {
        self.read().value.clone()
    }

    pub fn state(&self) -> LoadState {
        self.read().state.clone()
    }

    pub fn is_ready(&self) -> bool {
        self.read().state == LoadState::Ready
    }

    /// How many times a value has been put behind this handle: 0 before
    /// the first load, then one more for each successful load or reload.
    pub fn generation(&self) -> u64 {
        self.read().generation
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, SlotInner<T>> {
        // A panic while the lock was held leaves the slot's data as it was
        // before the write began; carry on with it.
        self.slot.inner.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// The type-erased side of a slot, so one cache holds every type.
trait AnySlot: Send + Sync {
    /// Read and decode. On a reload, a failure keeps the value there was.
    fn load(&self, source: &dyn Source) -> Result<(), ResourceError>;
    fn state(&self) -> LoadState;
    fn holders(&self) -> usize;
    fn as_any(&self) -> &dyn Any;
}

impl<T: ResourceType> AnySlot for Arc<Slot<T>> {
    fn load(&self, source: &dyn Source) -> Result<(), ResourceError> {
        let result = source
            .read(&self.path)
            .map_err(ResourceError::Read)
            .and_then(|bytes| decode::<T>(&bytes));
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(value) => {
                inner.value = Some(Arc::new(value));
                inner.state = LoadState::Ready;
                inner.generation += 1;
                Ok(())
            }
            Err(e) => {
                if inner.value.is_none() {
                    inner.state = LoadState::Failed(e.to_string());
                }
                Err(e)
            }
        }
    }

    fn state(&self) -> LoadState {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .state
            .clone()
    }

    fn holders(&self) -> usize {
        // One reference is the cache's own.
        Arc::strong_count(self) - 1
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

type Key = (TypeId, String);
type Listener = Box<dyn FnMut(&str) + Send>;

/// The cache of loaded resources.
#[derive(Default)]
pub struct Resources {
    slots: Mutex<HashMap<Key, Box<dyn AnySlot>>>,
    queue: Mutex<VecDeque<Key>>,
    listeners: Mutex<Vec<Listener>>,
}

impl Resources {
    pub fn new() -> Self {
        Resources::default()
    }

    /// The handle for `path`, loading it now if it has not been.
    pub fn load<T: ResourceType>(&self, source: &dyn Source, path: &str) -> Resource<T> {
        let (handle, fresh) = self.slot::<T>(path);
        if fresh || handle.state() == LoadState::Queued {
            let slots = self.lock_slots();
            if let Some(slot) = slots.get(&key::<T>(path))
                && let Err(e) = slot.load(source)
            {
                log::warn!("{path}: {e}");
            }
        }
        handle
    }

    /// The handle for `path`, without reading anything: a new one is queued
    /// for [`Resources::pump`].
    pub fn request<T: ResourceType>(&self, path: &str) -> Resource<T> {
        let (handle, fresh) = self.slot::<T>(path);
        if fresh {
            self.queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push_back(key::<T>(path));
        }
        handle
    }

    /// Load up to `budget` queued requests, oldest first. Returns how many
    /// were loaded, whether they succeeded or not.
    pub fn pump(&self, source: &dyn Source, budget: usize) -> usize {
        let mut done = 0;
        while done < budget {
            let Some(key) = self
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pop_front()
            else {
                break;
            };
            let slots = self.lock_slots();
            // Collected, or loaded directly, since it was queued.
            if let Some(slot) = slots.get(&key)
                && slot.state() == LoadState::Queued
                && let Err(e) = slot.load(source)
            {
                log::warn!("{}: {e}", key.1);
            }
            done += 1;
        }
        done
    }

    /// How many requests are waiting for [`Resources::pump`].
    pub fn queued(&self) -> usize {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Read `path` again for every type it has been loaded as, then tell the
    /// listeners. Returns how many handles got a new value.
    ///
    /// A reload that fails keeps the value there was, so a half-saved file
    /// in an editor does not turn the surface on screen into an error.
    pub fn reload(&self, source: &dyn Source, path: &str) -> usize {
        let mut reloaded = 0;
        {
            let slots = self.lock_slots();
            for ((_, p), slot) in slots.iter() {
                if p != path {
                    continue;
                }
                match slot.load(source) {
                    Ok(()) => reloaded += 1,
                    Err(e) => log::warn!("reloading {path}: {e}"),
                }
            }
        }
        if reloaded > 0 {
            for listener in self
                .listeners
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter_mut()
            {
                listener(path);
            }
        }
        reloaded
    }

    /// Call `f` with the path of every resource that reloads.
    pub fn on_reload(&self, f: impl FnMut(&str) + Send + 'static) {
        self.listeners
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Box::new(f));
    }

    /// Forget every resource no handle is held to. Returns how many.
    pub fn collect(&self) -> usize {
        let mut slots = self.lock_slots();
        let before = slots.len();
        slots.retain(|_, slot| slot.holders() > 0);
        before - slots.len()
    }

    /// How many resources are cached, loaded or not.
    pub fn len(&self) -> usize {
        self.lock_slots().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The handle for `path`, and whether it was made just now.
    fn slot<T: ResourceType>(&self, path: &str) -> (Resource<T>, bool) {
        let mut slots = self.lock_slots();
        let key = key::<T>(path);
        if let Some(existing) = slots.get(&key) {
            let slot = existing
                .as_any()
                .downcast_ref::<Arc<Slot<T>>>()
                .expect("a slot is keyed by its own type");
            return (
                Resource {
                    slot: Arc::clone(slot),
                },
                false,
            );
        }
        let slot = Arc::new(Slot {
            path: path.to_string(),
            inner: RwLock::new(SlotInner {
                state: LoadState::Queued,
                value: None,
                generation: 0,
            }),
        });
        slots.insert(key, Box::new(Arc::clone(&slot)));
        (Resource { slot }, true)
    }

    fn lock_slots(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Box<dyn AnySlot>>> {
        self.slots.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn key<T: 'static>(path: &str) -> Key {
    (TypeId::of::<T>(), path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceFile, ResourceView, tag};

    /// A resource that is its payload as text.
    #[derive(Debug, PartialEq)]
    struct Note(String);

    impl ResourceType for Note {
        const KIND: [u8; 4] = *b"NOTE";
        const VERSION: u32 = 1;
        fn decode(file: &ResourceView<'_>) -> Result<Self, ResourceError> {
            let data = file.require(tag::DATA)?;
            Ok(Note(String::from_utf8_lossy(data).into_owned()))
        }
    }

    fn note(text: &str) -> Vec<u8> {
        let mut f = ResourceFile::new(Note::KIND, 1);
        f.push(tag::DATA, text.as_bytes().to_vec());
        f.to_bytes()
    }

    fn files(pairs: &[(&str, Vec<u8>)]) -> HashMap<String, Vec<u8>> {
        pairs
            .iter()
            .map(|(p, b)| (p.to_string(), b.clone()))
            .collect()
    }

    #[test]
    fn one_path_is_read_once_and_shared() {
        let fs = files(&[("a.note", note("hello"))]);
        let res = Resources::new();
        let a = res.load::<Note>(&fs, "a.note");
        let b = res.load::<Note>(&HashMap::new(), "a.note");
        assert_eq!(a.get().unwrap().0, "hello");
        assert!(Arc::ptr_eq(&a.get().unwrap(), &b.get().unwrap()));
        assert_eq!(res.len(), 1);
    }

    #[test]
    fn a_missing_file_is_a_failed_handle_not_a_panic() {
        let res = Resources::new();
        let h = res.load::<Note>(&HashMap::new(), "gone.note");
        assert!(matches!(h.state(), LoadState::Failed(_)));
        assert!(h.get().is_none());
    }

    #[test]
    fn a_request_waits_for_the_pump() {
        let fs = files(&[("a.note", note("a")), ("b.note", note("b"))]);
        let res = Resources::new();
        let a = res.request::<Note>("a.note");
        let b = res.request::<Note>("b.note");
        assert_eq!(a.state(), LoadState::Queued);
        assert_eq!(res.queued(), 2);
        assert_eq!(res.pump(&fs, 1), 1);
        assert!(a.is_ready());
        assert_eq!(b.state(), LoadState::Queued);
        assert_eq!(res.pump(&fs, 10), 1);
        assert_eq!(b.get().unwrap().0, "b");
        assert_eq!(res.pump(&fs, 10), 0);
    }

    #[test]
    fn reload_swaps_the_value_and_says_so() {
        let mut fs = files(&[("a.note", note("one"))]);
        let res = Resources::new();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h2 = Arc::clone(&heard);
        res.on_reload(move |p| h2.lock().unwrap().push(p.to_string()));

        let h = res.load::<Note>(&fs, "a.note");
        let old = h.get().unwrap();
        assert_eq!(h.generation(), 1);

        fs.insert("a.note".into(), note("two"));
        assert_eq!(res.reload(&fs, "a.note"), 1);
        assert_eq!(h.get().unwrap().0, "two");
        assert_eq!(h.generation(), 2);
        assert_eq!(old.0, "one", "a value already taken is not pulled away");
        assert_eq!(*heard.lock().unwrap(), ["a.note"]);
    }

    #[test]
    fn a_failed_reload_keeps_the_old_value() {
        let mut fs = files(&[("a.note", note("one"))]);
        let res = Resources::new();
        let h = res.load::<Note>(&fs, "a.note");
        fs.insert("a.note".into(), b"KRES broken".to_vec());
        assert_eq!(res.reload(&fs, "a.note"), 0);
        assert_eq!(h.get().unwrap().0, "one");
        assert_eq!(h.state(), LoadState::Ready);
        assert_eq!(h.generation(), 1);
    }

    #[test]
    fn collect_frees_only_what_nobody_holds() {
        let fs = files(&[("a.note", note("a")), ("b.note", note("b"))]);
        let res = Resources::new();
        let kept = res.load::<Note>(&fs, "a.note");
        drop(res.load::<Note>(&fs, "b.note"));
        assert_eq!(res.collect(), 1);
        assert_eq!(res.len(), 1);
        assert!(kept.is_ready());
    }

    #[test]
    fn the_wrong_kind_is_refused() {
        let mut f = ResourceFile::new(*b"OTHR", 1);
        f.push(tag::DATA, vec![]);
        let fs = files(&[("a.note", f.to_bytes())]);
        let h = Resources::new().load::<Note>(&fs, "a.note");
        let LoadState::Failed(why) = h.state() else {
            panic!("{:?}", h.state())
        };
        assert!(why.contains("OTHR"), "{why}");
    }
}
