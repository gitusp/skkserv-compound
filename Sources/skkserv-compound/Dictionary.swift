// SPDX-License-Identifier: MIT

import Foundation

public enum DictionarySource: Sendable, Hashable {
    case user
    case system
}

public struct Candidate: Sendable, Hashable {
    public let text: String
    public let source: DictionarySource

    public init(text: String, source: DictionarySource) {
        self.text = text
        self.source = source
    }
}

public struct ParsedEntry: Sendable, Equatable {
    public let reading: String
    public let candidates: [String]
    public let isOkuriAri: Bool

    public init(reading: String, candidates: [String], isOkuriAri: Bool = false) {
        self.reading = reading
        self.candidates = candidates
        self.isOkuriAri = isOkuriAri
    }
}
