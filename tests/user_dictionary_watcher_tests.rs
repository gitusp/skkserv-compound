// SPDX-License-Identifier: MIT

use skkserv_compound::snapshot::DictionarySnapshot;
use skkserv_compound::store::DictionaryStore;
use skkserv_compound::watcher::UserDictionaryWatcher;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tempfile::TempDir;

async fn wait_until<F, Fut>(timeout: Duration, mut predicate: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    predicate().await
}

#[tokio::test]
async fn loads_on_start() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    std::fs::write(&path, "あ /亜/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    let snap = store.current();
    let texts: Vec<&str> = snap
        .candidates("あ")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(texts, vec!["亜"]);

    watcher.stop();
}

#[tokio::test]
async fn reindex_on_change() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    std::fs::write(&path, "あ /亜/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    {
        let mut handle = OpenOptions::new().append(true).open(&path).unwrap();
        handle.write_all("い /胃/\n".as_bytes()).unwrap();
    }

    let store_clone = store.clone();
    let updated = wait_until(Duration::from_secs(5), || {
        let store = store_clone.clone();
        async move {
            let snap = store.current();
            let texts: Vec<String> = snap
                .candidates("い")
                .iter()
                .map(|c| c.text.clone())
                .collect();
            texts == vec!["胃".to_string()]
        }
    })
    .await;
    assert!(updated, "expected user dict update to reach the store");

    watcher.stop();
}

#[tokio::test]
async fn keeps_snapshot_on_failure() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    std::fs::write(&path, "あ /亜/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    std::fs::write(&path, [0xC0u8, 0xAFu8, 0xFFu8]).unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    let snap = store.current();
    let texts: Vec<&str> = snap
        .candidates("あ")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(texts, vec!["亜"]);

    watcher.stop();
}

#[tokio::test]
async fn handles_atomic_rename() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    std::fs::write(&path, "あ /亜/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    {
        let tmp = dir.path().join("user.dict.tmp");
        std::fs::write(&tmp, "あ /亜/\nい /胃/\n").unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    }

    let store_clone = store.clone();
    let replaced = wait_until(Duration::from_secs(5), || {
        let store = store_clone.clone();
        async move {
            let snap = store.current();
            let texts: Vec<String> = snap
                .candidates("い")
                .iter()
                .map(|c| c.text.clone())
                .collect();
            texts == vec!["胃".to_string()]
        }
    })
    .await;
    assert!(replaced, "expected first rename to take effect");

    {
        let tmp = dir.path().join("user.dict.tmp");
        std::fs::write(&tmp, "あ /亜/\nい /胃/\nう /宇/\n").unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    }

    let store_clone = store.clone();
    let again = wait_until(Duration::from_secs(5), || {
        let store = store_clone.clone();
        async move {
            let snap = store.current();
            let texts: Vec<String> = snap
                .candidates("う")
                .iter()
                .map(|c| c.text.clone())
                .collect();
            texts == vec!["宇".to_string()]
        }
    })
    .await;
    assert!(again, "expected second rename to also reindex");

    watcher.stop();
}

struct StoreUpdateProbe {
    store: DictionaryStore,
    stop: Arc<AtomicBool>,
    records: Arc<Mutex<Vec<Record>>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone)]
struct Record {
    fingerprint: String,
    reading_count: usize,
}

impl StoreUpdateProbe {
    fn new(store: DictionaryStore) -> Self {
        Self {
            store,
            stop: Arc::new(AtomicBool::new(false)),
            records: Arc::new(Mutex::new(Vec::new())),
            handle: None,
        }
    }

    fn start(&mut self) {
        // Capture a baseline synchronously so the spawned task always has a
        // prior fingerprint to compare against, even if subsequent store
        // updates are coalesced before the task gets its first turn.
        {
            let snap = self.store.current();
            let fp = Self::fingerprint(&snap);
            self.records.lock().unwrap().push(Record {
                fingerprint: fp,
                reading_count: snap.entries_by_reading.len(),
            });
        }

        let store = self.store.clone();
        let stop = self.stop.clone();
        let records = self.records.clone();
        let handle = tokio::spawn(async move {
            while !stop.load(Ordering::Acquire) {
                let snap = store.current();
                let fp = Self::fingerprint(&snap);
                {
                    let mut recs = records.lock().unwrap();
                    let changed = recs.last().map(|r| r.fingerprint != fp).unwrap_or(true);
                    if changed {
                        recs.push(Record {
                            fingerprint: fp,
                            reading_count: snap.entries_by_reading.len(),
                        });
                    }
                }
                tokio::task::yield_now().await;
            }
        });
        self.handle = Some(handle);
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }

    fn records(&self) -> Vec<Record> {
        self.records.lock().unwrap().clone()
    }

    fn update_count(&self) -> usize {
        self.records.lock().unwrap().len().saturating_sub(1)
    }

    fn fingerprint(snapshot: &DictionarySnapshot) -> String {
        let mut entries: Vec<(&String, &Vec<skkserv_compound::dictionary::Candidate>)> =
            snapshot.entries_by_reading.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let parts: Vec<String> = entries
            .into_iter()
            .map(|(k, v)| {
                let texts: Vec<&str> = v.iter().map(|c| c.text.as_str()).collect();
                format!("{}={}", k, texts.join(","))
            })
            .collect();
        parts.join("|")
    }
}

fn append_line(path: &std::path::Path, line: &str) {
    let mut handle = OpenOptions::new().append(true).open(path).unwrap();
    handle.write_all(line.as_bytes()).unwrap();
    drop(handle);
}

#[tokio::test]
async fn coalesces_events_during_reindex() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    File::create(&path).unwrap();
    std::fs::write(&path, "あ /v0/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    let store_for_wait = store.clone();
    let bootstrapped = wait_until(Duration::from_secs(5), || {
        let store = store_for_wait.clone();
        async move {
            let snap = store.current();
            let t: Vec<String> = snap
                .candidates("あ")
                .iter()
                .map(|c| c.text.clone())
                .collect();
            t == vec!["v0".to_string()]
        }
    })
    .await;
    assert!(bootstrapped);

    let mut probe = StoreUpdateProbe::new(store.clone());
    probe.start();

    let write_count = 20usize;
    for i in 1..=write_count {
        append_line(&path, &format!("こ{} /v{}/\n", i, i));
    }
    let last_reading = format!("こ{}", write_count);
    let last_value = format!("v{}", write_count);

    let last_reading_clone = last_reading.clone();
    let last_value_clone = last_value.clone();
    let store_for_wait = store.clone();
    let landed = wait_until(Duration::from_secs(10), || {
        let store = store_for_wait.clone();
        let lr = last_reading_clone.clone();
        let lv = last_value_clone.clone();
        async move {
            let snap = store.current();
            let t: Vec<String> = snap
                .candidates(&lr)
                .iter()
                .map(|c| c.text.clone())
                .collect();
            t == vec![lv]
        }
    })
    .await;
    assert!(landed);

    let final_fp = StoreUpdateProbe::fingerprint(&store.current());
    let probe_records = probe.records.clone();
    let settled = wait_until(Duration::from_secs(2), || {
        let probe_records = probe_records.clone();
        let final_fp = final_fp.clone();
        async move {
            let recs = probe_records.lock().unwrap();
            recs.last().map(|r| r.fingerprint.clone()) == Some(final_fp)
        }
    })
    .await;
    assert!(settled);

    let updates = probe.update_count();
    assert!(updates >= 1, "expected at least one store update");
    assert!(
        updates < write_count,
        "expected coalescing: observed {} updates for {} writes",
        updates,
        write_count
    );

    let snap = store.current();
    let t: Vec<String> = snap
        .candidates(&last_reading)
        .iter()
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(t, vec![last_value]);
    let bootstrap: Vec<String> = snap
        .candidates("あ")
        .iter()
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(bootstrap, vec!["v0".to_string()]);

    probe.stop();
    watcher.stop();
}

#[tokio::test]
async fn reindex_is_single_flight_under_flood() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("user.dict");
    std::fs::write(&path, "あ /v0/\n").unwrap();

    let store = DictionaryStore::new();
    let watcher =
        UserDictionaryWatcher::new(path.to_str().unwrap().to_string(), vec![], store.clone());
    watcher.start().await.unwrap();

    let store_for_wait = store.clone();
    let bootstrapped = wait_until(Duration::from_secs(5), || {
        let store = store_for_wait.clone();
        async move {
            let snap = store.current();
            let t: Vec<String> = snap
                .candidates("あ")
                .iter()
                .map(|c| c.text.clone())
                .collect();
            t == vec!["v0".to_string()]
        }
    })
    .await;
    assert!(bootstrapped);

    let mut probe = StoreUpdateProbe::new(store.clone());
    probe.start();

    let write_count = 40usize;
    for i in 1..=write_count {
        append_line(&path, &format!("な{} /n{}/\n", i, i));
    }
    let last_reading = format!("な{}", write_count);
    let last_value = format!("n{}", write_count);

    let last_reading_clone = last_reading.clone();
    let last_value_clone = last_value.clone();
    let store_for_wait = store.clone();
    let landed = wait_until(Duration::from_secs(10), || {
        let store = store_for_wait.clone();
        let lr = last_reading_clone.clone();
        let lv = last_value_clone.clone();
        async move {
            let snap = store.current();
            let t: Vec<String> = snap
                .candidates(&lr)
                .iter()
                .map(|c| c.text.clone())
                .collect();
            t == vec![lv]
        }
    })
    .await;
    assert!(landed);

    let final_fp = StoreUpdateProbe::fingerprint(&store.current());
    let probe_records = probe.records.clone();
    let settled = wait_until(Duration::from_secs(2), || {
        let probe_records = probe_records.clone();
        let final_fp = final_fp.clone();
        async move {
            let recs = probe_records.lock().unwrap();
            recs.last().map(|r| r.fingerprint.clone()) == Some(final_fp)
        }
    })
    .await;
    assert!(settled);

    let records = probe.records();
    let updates = probe.update_count();
    assert!(updates >= 1, "expected at least one reindex landing");
    let monotonic = records
        .windows(2)
        .all(|w| w[1].reading_count >= w[0].reading_count);
    assert!(
        monotonic,
        "snapshot reading count regressed under flood: {:?}",
        records.iter().map(|r| r.reading_count).collect::<Vec<_>>()
    );

    let snap = store.current();
    let t: Vec<String> = snap
        .candidates(&last_reading)
        .iter()
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(t, vec![last_value]);
    let bootstrap: Vec<String> = snap
        .candidates("あ")
        .iter()
        .map(|c| c.text.clone())
        .collect();
    assert_eq!(bootstrap, vec!["v0".to_string()]);

    probe.stop();
    watcher.stop();
}
