// SPDX-License-Identifier: GPL-3.0-or-later WITH AdditionRef-Kerosene-Exception-1.0

//! Named counters for health checking.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// A set of named counters. Names are created on first use.
#[derive(Default)]
pub struct Health {
    counters: Mutex<BTreeMap<String, AtomicU64>>,
}

impl Health {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one to `id`, returning the new count.
    pub fn bump(&self, id: &str) -> u64 {
        let mut map = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(id.to_string())
            .or_default()
            .fetch_add(1, Ordering::Relaxed)
            + 1
    }

    /// Count an error under its id.
    pub fn record(&self, error: &impl crate::KError) -> u64 {
        self.bump(&error.id())
    }

    /// Count a recovery from `error`, as `kengineCount<Name>Recovery`
    /// (`kengineRenderError` becomes `kengineCountRenderIssueRecovery`).
    pub fn recovered(&self, error: &impl crate::KError) -> u64 {
        self.bump(&recovery_id(&error.id()))
    }

    pub fn get(&self, id: &str) -> u64 {
        let map = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        map.get(id).map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// Every counter, sorted by name.
    pub fn snapshot(&self) -> Vec<(String, u64)> {
        let map = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        map.iter()
            .map(|(k, v)| (k.clone(), v.load(Ordering::Relaxed)))
            .collect()
    }
}

/// The counter id that counts recoveries from the error `id`.
pub fn recovery_id(id: &str) -> String {
    let base = id.strip_suffix("Error").unwrap_or(id);
    match base.strip_prefix("kengine") {
        Some(name) => format!("kengineCount{name}IssueRecovery"),
        None => format!("{base}_CountRecovery"),
    }
}

/// The process-wide counters.
pub fn health() -> &'static Health {
    static HEALTH: OnceLock<Health> = OnceLock::new();
    HEALTH.get_or_init(Health::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineError;

    #[test]
    fn counts_errors_and_recoveries() {
        let h = Health::new();
        let e = EngineError::Render("lost".into());
        assert_eq!(h.record(&e), 1);
        assert_eq!(h.record(&e), 2);
        assert_eq!(h.recovered(&e), 1);
        assert_eq!(h.get("kengineRenderError"), 2);
        assert_eq!(h.get("kengineCountRenderIssueRecovery"), 1);
        assert_eq!(h.get("nothing"), 0);
        assert_eq!(h.snapshot().len(), 2);
    }
}
