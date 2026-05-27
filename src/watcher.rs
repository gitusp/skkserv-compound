// SPDX-License-Identifier: MIT

use crate::loader::DictionaryLoaderError;
use crate::loader::{expand_tilde, load_system_layer, load_user_layer};
use crate::snapshot::{DictionarySnapshot, Layer};
use crate::store::DictionaryStore;
use log::warn;
use notify::{Event, RecursiveMode, Watcher};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("UserDictionaryWatcher already started")]
    AlreadyStarted,
    #[error("user dictionary path '{0}' has no file name component")]
    PathHasNoFileName(String),
    #[error(transparent)]
    Notify(#[from] notify::Error),
    #[error(transparent)]
    Load(#[from] DictionaryLoaderError),
    #[error(transparent)]
    Join(#[from] tokio::task::JoinError),
}

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
    _worker: JoinHandle<()>,
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

    /// Installs the change watcher, loads the initial snapshot, and spawns
    /// the re-index worker task. The watcher is installed before the initial
    /// load so dictionary edits that land during the load are not lost.
    pub async fn start(&self) -> Result<(), WatcherError> {
        {
            let slot = self.runtime_lock();
            if slot.is_some() {
                return Err(WatcherError::AlreadyStarted);
            }
        }
        // Reset the stop flag so a start-after-stop cycle leaves the new
        // worker live instead of exiting on its first wake-up.
        self.stopped.store(false, Ordering::Release);

        // Install the watcher first: any user-dict change that lands during
        // the initial load will leave a pending notify permit for the worker
        // to pick up immediately, instead of being lost in a TOCTOU window.
        let watcher = self.install_watcher()?;

        // Build the immutable system tier exactly once. It is shared by `Arc`
        // for the life of the watcher; subsequent reindexes only rebuild the
        // (small) user tier, never re-reading the system dictionaries.
        let systems = self.system_dictionary_paths.clone();
        let system =
            Arc::new(tokio::task::spawn_blocking(move || load_system_layer(&systems)).await??);

        let user = self.user_dictionary_path.clone();
        let system_for_initial = system.clone();
        let user_layer =
            tokio::task::spawn_blocking(move || load_user_layer(&user, &system_for_initial))
                .await??;
        self.store
            .update(DictionarySnapshot::from_layers(user_layer, system.clone()));

        let worker = self.spawn_worker(system);

        let mut slot = self.runtime_lock();
        if slot.is_some() {
            // Another start() raced and won; abandon our setup.
            worker.abort();
            return Err(WatcherError::AlreadyStarted);
        }
        // If stop() landed during the initial load, it set `stopped` before
        // taking this same lock, so we observe it here and must NOT install:
        // installing now would leave a live FS watcher whose worker exits
        // immediately on `stopped` — a zombie that never reindexes and pins the
        // slot as `Some` so the watcher can never be restarted. Bailing while
        // holding the lock closes the race with stop()'s slot.take().
        if self.stopped.load(Ordering::Acquire) {
            worker.abort();
            // `watcher` (local) drops here, unsubscribing the FS watch.
            return Ok(());
        }
        *slot = Some(Runtime {
            _watcher: watcher,
            _worker: worker,
        });
        Ok(())
    }

    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        // Wake the worker so it can observe the stop flag and exit cleanly.
        // We do NOT call worker.abort(): aborting cancels the await on
        // spawn_blocking but cannot cancel the OS thread executing the
        // blocking load. Letting the worker observe `stopped` and return
        // gives it a chance to skip an in-flight reindex's store.update.
        self.notify.notify_one();
        let mut slot = self.runtime_lock();
        // Dropping Runtime drops the FS subscription (`_watcher`) and
        // detaches the worker JoinHandle (`_worker`); the detached worker
        // exits on the next loop iteration once it observes `stopped`.
        let _ = slot.take();
    }

    fn runtime_lock(&self) -> std::sync::MutexGuard<'_, Option<Runtime>> {
        self.runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn install_watcher(&self) -> Result<notify::RecommendedWatcher, WatcherError> {
        let target_path = expand_tilde(&self.user_dictionary_path);
        // Path::parent() returns Some(empty) for bare filenames like
        // "user.dict", which is unusable as a watch target. Fall back to the
        // current directory in both the None and empty cases.
        let parent: PathBuf = match target_path.parent() {
            Some(p) if !p.as_os_str().is_empty() => PathBuf::from(p),
            _ => PathBuf::from("."),
        };
        let target_filename: Option<OsString> = target_path.file_name().map(|n| n.to_os_string());
        if target_filename.is_none() {
            return Err(WatcherError::PathHasNoFileName(
                self.user_dictionary_path.clone(),
            ));
        }

        let notify = self.notify.clone();
        let target_filename_for_closure = target_filename.clone();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(event) => {
                    let matched = event.paths.iter().any(|p| {
                        p.file_name().map(|n| n.to_os_string()) == target_filename_for_closure
                    });
                    if matched {
                        notify.notify_one();
                    }
                }
                Err(e) => {
                    // Surface backend failures so a silently-broken watch is
                    // visible in the operator's log instead of stalling reindex.
                    warn!("file watcher backend error: {}", e);
                }
            })?;
        watcher.watch(&parent, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    }

    fn spawn_worker(&self, system: Arc<Layer>) -> JoinHandle<()> {
        let notify = self.notify.clone();
        let stopped = self.stopped.clone();
        let user = self.user_dictionary_path.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            loop {
                notify.notified().await;
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                // Only the user tier is rebuilt; the system tier is the shared
                // `Arc` cloned once per reindex (a refcount bump, not a reload).
                let user_clone = user.clone();
                let system_clone = system.clone();
                let res = tokio::task::spawn_blocking(move || {
                    load_user_layer(&user_clone, &system_clone)
                })
                .await;
                // Re-check stop before publishing: stop() may have been
                // called while the blocking load was in flight, and we do
                // not want to overwrite the store after the watcher is
                // logically shut down.
                if stopped.load(Ordering::Acquire) {
                    break;
                }
                match res {
                    Ok(Ok(user_layer)) => {
                        store.update(DictionarySnapshot::from_layers(user_layer, system.clone()))
                    }
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
