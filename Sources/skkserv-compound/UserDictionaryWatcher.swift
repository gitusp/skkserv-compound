// SPDX-License-Identifier: MIT

import Foundation
import Logging

public final class UserDictionaryWatcher: @unchecked Sendable {
    private let userDictionaryPath: String
    private let systemDictionaryPaths: [String]
    private let store: DictionaryStore
    private let logger: Logger

    private let watchQueue = DispatchQueue(label: "io.github.gitusp.skkserv-compound.watch")
    private let workQueue = DispatchQueue(label: "io.github.gitusp.skkserv-compound.reindex")
    private let stateLock = NSLock()

    private var source: DispatchSourceFileSystemObject?

    private var inFlight = false
    private var pending = false
    private var stopped = false

    public init(
        userDictionaryPath: String,
        systemDictionaryPaths: [String],
        store: DictionaryStore,
        logger: Logger
    ) {
        self.userDictionaryPath = userDictionaryPath
        self.systemDictionaryPaths = systemDictionaryPaths
        self.store = store
        self.logger = logger
    }

    /// Loads the initial snapshot synchronously and then installs the change watcher.
    public func start() async throws {
        let snapshot = try DictionaryLoader.loadSnapshot(
            userDictionaryPath: userDictionaryPath,
            systemDictionaryPaths: systemDictionaryPaths
        )
        await store.update(snapshot)
        installSource()
    }

    public func stop() {
        stateLock.lock()
        stopped = true
        stateLock.unlock()
        cancelSourceLocked()
    }

    private func cancelSourceLocked() {
        if let src = source {
            src.cancel()
            source = nil
        }
    }

    private func installSource() {
        let expanded = (userDictionaryPath as NSString).expandingTildeInPath
        let openedFd = open(expanded, O_EVTONLY)
        if openedFd < 0 {
            logger.warning("Could not open user dictionary for watching: \(expanded)")
            return
        }
        let src = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: openedFd,
            eventMask: [.write, .delete, .rename, .extend],
            queue: watchQueue
        )
        src.setEventHandler { [weak self] in
            guard let self else { return }
            let mask = src.data
            let replaced = mask.contains(.delete) || mask.contains(.rename)
            self.scheduleReindex(reinstall: replaced)
        }
        src.setCancelHandler { [openedFd] in
            close(openedFd)
        }
        source = src
        src.resume()
    }

    private func scheduleReindex(reinstall: Bool) {
        stateLock.lock()
        if stopped {
            stateLock.unlock()
            return
        }
        if inFlight {
            pending = true
            stateLock.unlock()
            if reinstall {
                reinstallWatcher()
            }
            return
        }
        inFlight = true
        stateLock.unlock()

        if reinstall {
            reinstallWatcher()
        }

        workQueue.async { [weak self] in
            self?.runReindex()
        }
    }

    private func reinstallWatcher() {
        watchQueue.async { [weak self] in
            guard let self else { return }
            self.stateLock.lock()
            let stopped = self.stopped
            self.stateLock.unlock()
            if stopped { return }
            if let src = self.source {
                src.cancel()
                self.source = nil
            }
            self.installSource()
        }
    }

    private func runReindex() {
        do {
            let snapshot = try DictionaryLoader.loadSnapshot(
                userDictionaryPath: userDictionaryPath,
                systemDictionaryPaths: systemDictionaryPaths
            )
            let store = self.store
            Task { await store.update(snapshot) }
        } catch {
            logger.warning("reindex failed; keeping previous snapshot: \(error)")
        }

        stateLock.lock()
        let runAgain = pending
        pending = false
        if !runAgain {
            inFlight = false
        }
        stateLock.unlock()

        if runAgain {
            workQueue.async { [weak self] in
                self?.runReindex()
            }
        }
    }
}
