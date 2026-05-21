// SPDX-License-Identifier: MIT

import Foundation

public enum DictionaryLoaderError: Error, CustomStringConvertible {
    case encodingNotRecognized(path: String)

    public var description: String {
        switch self {
        case .encodingNotRecognized(let path):
            return "Could not decode dictionary file as UTF-8 or EUC-JP: \(path)"
        }
    }
}

public enum DictionaryLoader {
    public static func loadSnapshot(userDictionaryPath: String, systemDictionaryPaths: [String]) throws -> DictionarySnapshot {
        let userParsed = DictionaryParser.parse(try readDictionaryFile(at: userDictionaryPath))
        var systemParsed: [ParsedEntry] = []
        for path in systemDictionaryPaths {
            systemParsed.append(contentsOf: DictionaryParser.parse(try readDictionaryFile(at: path)))
        }
        return buildSnapshot(user: userParsed, system: systemParsed)
    }

    public static func buildSnapshot(user: [ParsedEntry], system: [ParsedEntry]) -> DictionarySnapshot {
        var userNashi: [ParsedEntry] = []
        var userAri: [ParsedEntry] = []
        for entry in user {
            if entry.isOkuriAri { userAri.append(entry) } else { userNashi.append(entry) }
        }
        var systemNashi: [ParsedEntry] = []
        var systemAri: [ParsedEntry] = []
        for entry in system {
            if entry.isOkuriAri { systemAri.append(entry) } else { systemNashi.append(entry) }
        }
        return DictionarySnapshot(
            orderedEntries: mergeBucket(user: userNashi, system: systemNashi),
            orderedOkuriAriEntries: mergeBucket(user: userAri, system: systemAri)
        )
    }

    private static func mergeBucket(
        user: [ParsedEntry],
        system: [ParsedEntry]
    ) -> [(reading: String, candidates: [Candidate])] {
        struct Group {
            var candidates: [Candidate] = []
            var seen: Set<String> = []
        }
        var groups: [String: Group] = [:]
        // Reading order: first-appearance from user, then new readings from system.
        var order: [String] = []

        func ingest(_ entries: [ParsedEntry], source: DictionarySource) {
            for entry in entries {
                var group = groups[entry.reading]
                if group == nil {
                    group = Group()
                    order.append(entry.reading)
                }
                var g = group!
                for text in entry.candidates where g.seen.insert(text).inserted {
                    g.candidates.append(Candidate(text: text, source: source))
                }
                groups[entry.reading] = g
            }
        }

        ingest(user, source: .user)
        ingest(system, source: .system)

        return order.compactMap { reading in
            let group = groups[reading]!
            return group.candidates.isEmpty ? nil : (reading, group.candidates)
        }
    }

    static func readDictionaryFile(at path: String) throws -> String {
        let expanded = (path as NSString).expandingTildeInPath
        let url = URL(fileURLWithPath: expanded)
        let data = try Data(contentsOf: url)
        if let s = String(data: data, encoding: .utf8) {
            return s
        }
        if let s = String(data: data, encoding: .japaneseEUC) {
            return s
        }
        throw DictionaryLoaderError.encodingNotRecognized(path: path)
    }
}
