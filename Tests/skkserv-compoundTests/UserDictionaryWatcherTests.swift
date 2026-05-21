// SPDX-License-Identifier: MIT

import Testing
import Foundation
import Logging
@testable import skkserv_compound

@Suite("UserDictionaryWatcher")
struct UserDictionaryWatcherTests {
    private static let testLogger = Logger(label: "watcher-test")

    private func makeTempDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("skkserv-compound-watcher-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private func waitUntil(timeout: TimeInterval = 5.0, _ predicate: () async -> Bool) async -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if await predicate() { return true }
            try? await Task.sleep(nanoseconds: 50_000_000)
        }
        return await predicate()
    }

    @Test("起動時にユーザー辞書を同期ロードする")
    func loadsOnStart() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /亜/\n".write(to: url, atomically: true, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        #expect(await store.current().candidates(for: "あ").map(\.text) == ["亜"])
    }

    @Test("ファイル変更で reindex し snapshot を差し替える")
    func reindexOnChange() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /亜/\n".write(to: url, atomically: true, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        // Write extra candidate using append (in-place write, triggers .write event).
        let handle = try FileHandle(forWritingTo: url)
        try handle.seekToEnd()
        try handle.write(contentsOf: Data("い /胃/\n".utf8))
        try handle.close()

        let updated = await waitUntil {
            await store.current().candidates(for: "い").map(\.text) == ["胃"]
        }
        #expect(updated)
    }

    @Test("reindex 失敗時は旧 snapshot を維持する")
    func keepsSnapshotOnFailure() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /亜/\n".write(to: url, atomically: true, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        // Replace the file with bytes that the loader cannot decode.
        // A reliable way: delete then re-create as a directory at the path.
        // We use bytes that are invalid for both UTF-8 and EUC-JP. We pick a few that
        // are illegal continuation patterns under both encodings; if some platform
        // tolerates them we still pass since the snapshot becomes empty/changed in a
        // way that does not include "亜". So we assert the old snapshot is preserved.
        try Data([0xC0, 0xAF, 0xFF]).write(to: url)

        // Wait briefly to let the watcher attempt a reindex.
        try await Task.sleep(nanoseconds: 300_000_000)
        // Old snapshot must still be available.
        #expect(await store.current().candidates(for: "あ").map(\.text) == ["亜"])
    }

    @Test("atomic rename で watcher を張り直して反映できる")
    func handlesAtomicRename() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /亜/\n".write(to: url, atomically: true, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        // Foundation's atomic write replaces the file via rename.
        try "あ /亜/\nい /胃/\n".write(to: url, atomically: true, encoding: .utf8)

        let replaced = await waitUntil {
            await store.current().candidates(for: "い").map(\.text) == ["胃"]
        }
        #expect(replaced)

        // After re-installing the watcher, a follow-up edit must still trigger a reindex.
        try "あ /亜/\nい /胃/\nう /宇/\n".write(to: url, atomically: true, encoding: .utf8)
        let again = await waitUntil {
            await store.current().candidates(for: "う").map(\.text) == ["宇"]
        }
        #expect(again)
    }

    @Test("reindex 中の追加イベントは完了後にまとめて再実行する")
    func coalescesEventsDuringReindex() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /v0/\n".write(to: url, atomically: false, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        // Be sure the initial snapshot is in place before we begin observing.
        let bootstrapped = await waitUntil {
            await store.current().candidates(for: "あ").map(\.text) == ["v0"]
        }
        #expect(bootstrapped)

        let probe = StoreUpdateProbe(store: store)
        probe.start()
        defer { probe.stop() }

        // Issue a rapid burst of in-place appends. Each one is a separate file write
        // syscall on the same inode, and the watcher's DispatchSource fires .write
        // events for them. We expect the watcher to coalesce the events that arrive
        // during an in-flight reindex into a single follow-up reindex.
        let writeCount = 20
        for i in 1...writeCount {
            let handle = try FileHandle(forWritingTo: url)
            try handle.seekToEnd()
            try handle.write(contentsOf: Data("こ\(i) /v\(i)/\n".utf8))
            try handle.close()
        }
        let lastReading = "こ\(writeCount)"
        let lastValue = "v\(writeCount)"

        // Wait until the latest write is reflected in the store.
        let landed = await waitUntil(timeout: 10.0) {
            await store.current().candidates(for: lastReading).map(\.text) == [lastValue]
        }
        #expect(landed)

        // Wait for the probe to settle on the final fingerprint without depending on
        // an absolute sleep duration.
        let finalFingerprint = StoreUpdateProbe.fingerprint(of: await store.current())
        let settled = await waitUntil(timeout: 2.0) {
            probe.observedRecords.last?.fingerprint == finalFingerprint
        }
        #expect(settled)

        let updates = probe.observedUpdateCount
        #expect(updates >= 1, "expected at least one update to land in the store")
        #expect(updates < writeCount, "expected coalescing: observed \(updates) updates for \(writeCount) writes")

        // The final snapshot must reflect the very last write, plus the bootstrap entry.
        let final = await store.current()
        #expect(final.candidates(for: lastReading).map(\.text) == [lastValue])
        #expect(final.candidates(for: "あ").map(\.text) == ["v0"])
    }

    @Test("flood する書き込みでも reindex は単発に絞られる")
    func reindexIsSingleFlightUnderFlood() async throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("user.dict")
        try "あ /v0/\n".write(to: url, atomically: false, encoding: .utf8)

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: url.path,
            systemDictionaryPaths: [],
            store: store,
            logger: Self.testLogger
        )
        try await watcher.start()
        defer { watcher.stop() }

        let bootstrapped = await waitUntil {
            await store.current().candidates(for: "あ").map(\.text) == ["v0"]
        }
        #expect(bootstrapped)

        let probe = StoreUpdateProbe(store: store)
        probe.start()
        defer { probe.stop() }

        // Flood the watcher with a large number of rapid events. If the watcher were
        // to spawn a fresh reindex per event in parallel, we would expect either a
        // racy regression in observed snapshots, or many more update landings than
        // the watcher's single-flight + pending protocol can produce.
        let writeCount = 40
        for i in 1...writeCount {
            let handle = try FileHandle(forWritingTo: url)
            try handle.seekToEnd()
            try handle.write(contentsOf: Data("な\(i) /n\(i)/\n".utf8))
            try handle.close()
        }
        let lastReading = "な\(writeCount)"
        let lastValue = "n\(writeCount)"

        let landed = await waitUntil(timeout: 10.0) {
            await store.current().candidates(for: lastReading).map(\.text) == [lastValue]
        }
        #expect(landed)

        let finalFingerprint = StoreUpdateProbe.fingerprint(of: await store.current())
        let settled = await waitUntil(timeout: 2.0) {
            probe.observedRecords.last?.fingerprint == finalFingerprint
        }
        #expect(settled)

        let records = probe.observedRecords
        let updates = probe.observedUpdateCount
        // At least one reindex must have landed (the watcher must observe the flood).
        #expect(updates >= 1, "expected at least one reindex landing")
        // No parallel reindex would mean snapshots only grow as the file is appended to:
        // a parallel reindex finishing out-of-order could land an older, smaller snapshot
        // after a newer one. Assert monotonic growth in reading count.
        let monotonic = zip(records, records.dropFirst()).allSatisfy { prev, next in
            next.readingCount >= prev.readingCount
        }
        #expect(monotonic, "snapshot reading count regressed under flood: \(records.map(\.readingCount))")

        // The final snapshot must reflect the very last write, plus the bootstrap entry.
        let final = await store.current()
        #expect(final.candidates(for: lastReading).map(\.text) == [lastValue])
        #expect(final.candidates(for: "あ").map(\.text) == ["v0"])
    }
}

/// Thin test-only wrapper that observes the `DictionaryStore` from the outside and
/// records every distinct snapshot fingerprint that lands in it. Because
/// `DictionaryStore` is an `actor` and `UserDictionaryWatcher` holds a concrete
/// reference to it, we cannot intercept `update(_:)` calls directly without touching
/// the production code. Instead, a background detached task polls `current()` in a
/// tight loop and appends a record whenever the fingerprint changes — giving a
/// lower bound on the number of `update` calls the watcher made.
private final class StoreUpdateProbe: @unchecked Sendable {
    struct Record {
        let fingerprint: String
        let readingCount: Int
    }

    private let store: DictionaryStore
    private let lock = NSLock()
    private var _records: [Record] = []
    private var task: Task<Void, Never>?

    init(store: DictionaryStore) {
        self.store = store
    }

    func start() {
        let store = self.store
        task = Task.detached(priority: .userInitiated) { [weak self] in
            while !Task.isCancelled {
                let snapshot = await store.current()
                guard let self else { return }
                self.recordIfChanged(snapshot: snapshot)
                await Task.yield()
            }
        }
    }

    func stop() {
        task?.cancel()
        task = nil
    }

    var observedRecords: [Record] {
        lock.lock()
        defer { lock.unlock() }
        return _records
    }

    var observedUpdateCount: Int {
        // First record is the initial snapshot the probe saw on its first poll, not
        // an update — so update count is one less than the number of recorded
        // distinct fingerprints.
        max(0, observedRecords.count - 1)
    }

    private func recordIfChanged(snapshot: DictionarySnapshot) {
        let fp = StoreUpdateProbe.fingerprint(of: snapshot)
        lock.lock()
        if _records.last?.fingerprint != fp {
            _records.append(Record(fingerprint: fp, readingCount: snapshot.entriesByReading.count))
        }
        lock.unlock()
    }

    static func fingerprint(of snapshot: DictionarySnapshot) -> String {
        snapshot.entriesByReading
            .sorted(by: { $0.key < $1.key })
            .map { "\($0.key)=\($0.value.map(\.text).joined(separator: ","))" }
            .joined(separator: "|")
    }
}
