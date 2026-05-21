// SPDX-License-Identifier: MIT

import Foundation

public actor DictionaryStore {
    private var snapshot: DictionarySnapshot

    public init(initial: DictionarySnapshot = .empty) {
        self.snapshot = initial
    }

    public func update(_ snapshot: DictionarySnapshot) {
        self.snapshot = snapshot
    }

    public func current() -> DictionarySnapshot {
        snapshot
    }
}
