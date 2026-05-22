// SPDX-License-Identifier: MIT

use crate::snapshot::DictionarySnapshot;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct DictionaryStore {
    inner: Arc<RwLock<Arc<DictionarySnapshot>>>,
}

impl DictionaryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_initial(snapshot: DictionarySnapshot) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Arc::new(snapshot))),
        }
    }

    pub fn update(&self, snapshot: DictionarySnapshot) {
        // Recover from a poisoned lock rather than cascading the panic to
        // every subsequent request: the lock only ever guards a pointer swap,
        // so the inner state is always consistent regardless of where a
        // previous panic occurred.
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Arc::new(snapshot);
    }

    pub fn current(&self) -> Arc<DictionarySnapshot> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

impl Default for DictionaryStore {
    fn default() -> Self {
        Self::with_initial(DictionarySnapshot::empty())
    }
}
