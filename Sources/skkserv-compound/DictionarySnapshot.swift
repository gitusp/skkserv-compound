// SPDX-License-Identifier: MIT

import Foundation

public struct DictionarySnapshot: Sendable {
    public let entriesByReading: [String: [Candidate]]
    public let readingsByFirstCharacter: [Character: [String]]
    public let okuriAriEntriesByReading: [String: [Candidate]]
    public let okuriAriReadingsByFirstCharacter: [Character: [String]]

    public init(
        orderedEntries: [(reading: String, candidates: [Candidate])],
        orderedOkuriAriEntries: [(reading: String, candidates: [Candidate])] = []
    ) {
        (entriesByReading, readingsByFirstCharacter) = Self.index(orderedEntries)
        (okuriAriEntriesByReading, okuriAriReadingsByFirstCharacter) = Self.index(orderedOkuriAriEntries)
    }

    private static func index(
        _ entries: [(reading: String, candidates: [Candidate])]
    ) -> (byReading: [String: [Candidate]], byFirstChar: [Character: [String]]) {
        var byReading: [String: [Candidate]] = [:]
        var byFirstChar: [Character: [String]] = [:]
        for entry in entries {
            byReading[entry.reading] = entry.candidates
            if let first = entry.reading.first {
                byFirstChar[first, default: []].append(entry.reading)
            }
        }
        return (byReading, byFirstChar)
    }

    public func candidates(for reading: String) -> [Candidate] {
        entriesByReading[reading] ?? []
    }

    public func readings(startingWith first: Character) -> [String] {
        readingsByFirstCharacter[first] ?? []
    }

    public func okuriAriCandidates(for reading: String) -> [Candidate] {
        okuriAriEntriesByReading[reading] ?? []
    }

    /// Every okuri-nashi reading that matches a prefix of `chars[start...]`.
    func prefixMatches(in chars: [Character], from start: Int) -> [(length: Int, reading: String)] {
        guard start < chars.count,
              let candidates = readingsByFirstCharacter[chars[start]] else { return [] }
        let remaining = chars.count - start
        var result: [(length: Int, reading: String)] = []
        for reading in candidates {
            let length = reading.count
            if length > remaining { continue }
            if Self.matchesPrefix(reading, in: chars, from: start) {
                result.append((length, reading))
            }
        }
        return result
    }

    /// Every okuri-ari reading whose hiragana stem matches a prefix of
    /// `chars[start...]` AND whose trailing ASCII letter equals `okuriPrefix`.
    /// `length` covers only the hiragana stem (the ASCII letter is in the key,
    /// not in the input).
    func okuriAriPrefixMatches(
        in chars: [Character],
        from start: Int,
        okuriPrefix: Character
    ) -> [(length: Int, reading: String)] {
        guard start < chars.count,
              let candidates = okuriAriReadingsByFirstCharacter[chars[start]] else { return [] }
        let remaining = chars.count - start
        var result: [(length: Int, reading: String)] = []
        for reading in candidates {
            guard reading.last == okuriPrefix else { continue }
            let stemLength = reading.count - 1
            if stemLength < 1 || stemLength > remaining { continue }
            if Self.matchesPrefix(reading.dropLast(), in: chars, from: start) {
                result.append((stemLength, reading))
            }
        }
        return result
    }

    private static func matchesPrefix<S: Sequence>(_ prefix: S, in chars: [Character], from start: Int) -> Bool
    where S.Element == Character {
        var i = start
        for ch in prefix {
            if chars[i] != ch { return false }
            i += 1
        }
        return true
    }

    public static let empty = DictionarySnapshot(orderedEntries: [])
}
