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
        let mut guard = self.inner.write().expect("DictionaryStore lock poisoned");
        *guard = Arc::new(snapshot);
    }

    pub fn current(&self) -> Arc<DictionarySnapshot> {
        self.inner
            .read()
            .expect("DictionaryStore lock poisoned")
            .clone()
    }
}

impl Default for DictionaryStore {
    fn default() -> Self {
        Self::with_initial(DictionarySnapshot::empty())
    }
}
