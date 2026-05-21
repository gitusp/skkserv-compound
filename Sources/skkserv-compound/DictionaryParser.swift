// SPDX-License-Identifier: MIT

import Foundation

public enum DictionaryParser {
    public static func parse(_ source: String) -> [ParsedEntry] {
        var result: [ParsedEntry] = []
        source.enumerateLines { line, _ in
            if let entry = parseLine(line) {
                result.append(entry)
            }
        }
        return result
    }

    static func parseLine(_ raw: String) -> ParsedEntry? {
        var line = raw
        if line.hasSuffix("\r") {
            line = String(line.dropLast())
        }
        if line.isEmpty { return nil }
        if line.hasPrefix(";") { return nil }

        guard let space = line.firstIndex(of: " ") else { return nil }
        let reading = String(line[..<space])
        if reading.isEmpty { return nil }
        // SKK encodes okuri-ari headwords as `<hiragana>+<ASCII lowercase>`
        // (e.g. `おくr`). All-ASCII readings like `mini` are abbrevs and stay in
        // the okuri-nashi bucket.
        let okuriAri = trailingOkuri(of: reading) != nil

        let rest = line[line.index(after: space)...]
        guard rest.hasPrefix("/"), rest.hasSuffix("/") else { return nil }
        let inner = rest.dropFirst().dropLast()
        if inner.isEmpty { return nil }

        var seen = Set<String>()
        var texts: [String] = []
        for raw in inner.split(separator: "/", omittingEmptySubsequences: false) {
            var text = String(raw)
            if let semi = text.firstIndex(of: ";") {
                text = String(text[..<semi])
            }
            if text.isEmpty { continue }
            if seen.insert(text).inserted {
                texts.append(text)
            }
        }
        if texts.isEmpty { return nil }
        return ParsedEntry(reading: reading, candidates: texts, isOkuriAri: okuriAri)
    }

    /// Returns the trailing ASCII lowercase letter if `text` ends with
    /// `<hiragana><a-z>`, otherwise nil. Hiragana range U+3041..U+3096 matches
    /// the SKK okuri-ari stem set.
    static func trailingOkuri(of text: String) -> Character? {
        let scalars = text.unicodeScalars
        guard let last = scalars.last, ("a"..."z").contains(Character(last)) else { return nil }
        guard let before = scalars.dropLast().last,
              (0x3041...0x3096).contains(before.value) else { return nil }
        return Character(last)
    }
}
