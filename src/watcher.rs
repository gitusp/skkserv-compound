// SPDX-License-Identifier: MIT

use crate::loader::{expand_tilde, load_snapshot};
use crate::store::DictionaryStore;
use anyhow::Result;
use notify::{Event, RecursiveMode, Watcher};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::warn;

/// Loads the user/system dictionaries once at start, then re-loads on every
/// detected change to the user dictionary file. Re-indexes are single-flight:
/// events that arrive during an in-flight re-index are coalesced into one
/// follow-up run.
pub struct UserDictionaryWatcher {
    user_dictionary_path: String,
    system_dictionary_paths: Vec<String>,
    store: DictionaryStore,
    notify: Arc<Notify>,
    stopped: Arc<AtomicBool>,
    runtime: std::sync::Mutex<Option<Runtime>>,
}

struct Runtime {
    _watcher: notify::RecommendedWatcher,
    worker: JoinHandle<()>,
}

impl UserDictionaryWatcher {
    pub fn new(
        user_dictionary_path: String,
        system_dictionary_paths: Vec<String>,
        store: DictionaryStore,
    ) -> Self {
        Self {
            user_dictionary_path,
            system_dictionary_paths,
            store,
            notify: Arc::new(Notify::new()),
            stopped: Arc::new(AtomicBool::new(false)),
            runtime: std::sync::Mutex::new(None),
        }
    }

    /// Loads the initial snapshot synchronously, installs the change watcher,
    /// and spawns the re-index worker task.
    pub async fn start(&self) -> Result<()> {
        let user = self.user_dictionary_path.clone();
        let systems = self.system_dictionary_paths.clone();
        let snapshot =
            tokio::task::spawn_blocking(move || load_snapshot(&user, &systems)).await??;
        self.store.update(snapshot);

        let watcher = self.install_watcher()?;
        let worker = self.spawn_worker();

        let mut slot = self
            .runtime
            .lock()
            .expect("UserDictionaryWatcher runtime lock poisoned");
        *slot = Some(Runtime {
            _watcher: watcher,
            worker,
        });
        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        // Wake the worker so it can observe the stop flag.
        self.notify.notify_one();
        if let Ok(mut slot) = self.runtime.lock()
            && let Some(rt) = slot.take()
        {
            rt.worker.abort();
            // _watcher is dropped here, releasing the FS subscription.
        }
    }

    fn install_watcher(&self) -> Result<notify::RecommendedWatcher> {
        let target_path = expand_tilde(&self.user_dictionary_path);
        let parent: PathBuf = target_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let target_filename: Option<OsString> = target_path.file_name().map(|n| n.to_os_string());

        let notify = self.notify.clone();
        let target_filename_for_closure = target_filename.clone();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let matched = event.paths.iter().any(|p| {
                    p.file_name().map(|n| n.to_os_string()) == target_filename_for_closure
                });
                if matched {
                    notify.notify_one();
                }
            }
        })?;
        watcher.watch(&parent, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    }

    fn spawn_worker(&self) -> JoinHandle<()> {
        let notify = self.notify.clone();
        let stopped = self.stopped.clone();
        let user = self.user_dictionary_path.clone();
        let systems = self.system_dictionary_paths.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            loop {
                notify.notified().await;
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                let user_clone = user.clone();
                let systems_clone = systems.clone();
                let res =
                    tokio::task::spawn_blocking(move || load_snapshot(&user_clone, &systems_clone))
                        .await;
                match res {
                    Ok(Ok(snapshot)) => store.update(snapshot),
                    Ok(Err(e)) => warn!("reindex failed; keeping previous snapshot: {}", e),
                    Err(e) => warn!("reindex task panicked: {}", e),
                }
            }
        })
    }
}

impl Drop for UserDictionaryWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}
