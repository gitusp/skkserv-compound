// SPDX-License-Identifier: MIT

import Foundation

public struct CompoundGeneratorConfig: Sendable {
    public var maxCandidatesPerReading: Int
    public var maxFinalCandidates: Int

    public init(maxCandidatesPerReading: Int = 5, maxFinalCandidates: Int = 10) {
        self.maxCandidatesPerReading = maxCandidatesPerReading
        self.maxFinalCandidates = maxFinalCandidates
    }
}

public enum CompoundGenerator {
    public static func generate(
        yomi: String,
        snapshot: DictionarySnapshot,
        config: CompoundGeneratorConfig = CompoundGeneratorConfig(),
        okuriPrefix: String? = nil
    ) -> [String] {
        let chars = Array(yomi)
        let n = chars.count
        // A compound by definition requires at least two reading parts. The same
        // floor applies in okuri-ari mode: at least one okuri-nashi prefix part
        // plus one okuri-ari stem part.
        if n < 2 { return [] }

        // okuri-ari mode is opted into when the SKK request carried a trailing
        // okurigana romaji marker (e.g. `もんだいなs` → body `もんだいな` + `s`).
        // The marker drives the dictionary bucket used for the last split part.
        let okuriChar: Character? = okuriPrefix.flatMap { $0.first }

        var seen = Set<String>()
        var result: [String] = []
        result.reserveCapacity(config.maxFinalCandidates)

        // The outer best-first axis is k, the number of reading parts. k = 1 is
        // intentionally skipped: single-word exact matches are returned by the SKK
        // client's own dictionary lookup, not by this compound server. We expand k
        // upward only while the dedupe'd output is still short of the final cap, so
        // larger k stages are never enumerated when smaller k already fills the cap.
        var k = 2
        while k <= n && result.count < config.maxFinalCandidates {
            let splits = enumerateSplits(
                k: k,
                chars: chars,
                snapshot: snapshot,
                cap: config.maxCandidatesPerReading,
                okuriPrefix: okuriChar
            )
            if !splits.isEmpty {
                expand(splits: splits, finalCap: config.maxFinalCandidates, seen: &seen, result: &result)
            }
            k += 1
        }

        return result
    }

    private static func expand(splits unsorted: [SplitInfo], finalCap: Int, seen: inout Set<String>, result: inout [String]) {
        // All splits in this batch share the same numParts; pre-sort by the
        // within-k priority key so that smaller splitIdx already encodes
        // (minPartLen DESC, enumOrder ASC). The heap comparator then only needs
        // splitIdx as the final tiebreaker.
        let splits = unsorted.sorted { a, b in compareSplitKey(a, b) < 0 }

        // Min-heap priority, in order:
        //   1. minPartLen DESC — splits with a longer shortest part stay strictly
        //      above splits with a shorter shortest part (a genuine quality signal:
        //      single-kana fragments tend to produce noise).
        //   2. rank ASC — within the same minPartLen tier, every split contributes
        //      its k-th best candidate before any split contributes its (k+1)-th.
        //      This is the round-robin axis that prevents one split from monopolising
        //      the final cap when several splits share the same minPartLen.
        //   3. splitIdx ASC — deterministic fallback, encoded by the pre-sort above.
        var heap = MinHeap<PQEntry> { lhs, rhs in
            let infoL = splits[lhs.splitIdx]
            let infoR = splits[rhs.splitIdx]
            if infoL.minPartLen != infoR.minPartLen { return infoL.minPartLen > infoR.minPartLen }
            if lhs.rank != rhs.rank { return lhs.rank < rhs.rank }
            return lhs.splitIdx < rhs.splitIdx
        }

        for (idx, info) in splits.enumerated() {
            let indices = [Int](repeating: 0, count: info.numParts)
            let text = buildText(info: info, indices: indices)
            heap.push(PQEntry(splitIdx: idx, rank: 0, indices: indices, text: text))
        }

        while result.count < finalCap {
            guard let entry = heap.pop() else { break }
            if seen.insert(entry.text).inserted {
                result.append(entry.text)
                if result.count >= finalCap { return }
            }
            // Push the next combination within this split, in lex order with the
            // rightmost index varying fastest. rank increments by one so this
            // entry queues behind every other split's same-rank candidate.
            let info = splits[entry.splitIdx]
            var next = entry.indices
            var i = info.numParts - 1
            while i >= 0 {
                next[i] += 1
                if next[i] < info.partCandidates[i].count { break }
                next[i] = 0
                i -= 1
            }
            if i >= 0 {
                let text = buildText(info: info, indices: next)
                heap.push(PQEntry(splitIdx: entry.splitIdx, rank: entry.rank + 1, indices: next, text: text))
            }
        }
    }

    private static func enumerateSplits(
        k: Int,
        chars: [Character],
        snapshot: DictionarySnapshot,
        cap: Int,
        okuriPrefix: Character?
    ) -> [SplitInfo] {
        var splits: [SplitInfo] = []
        var enumOrder = 0
        var parts: [String] = []
        parts.reserveCapacity(k)
        enumerateRecursive(
            k: k, depth: 0, start: 0, chars: chars,
            snapshot: snapshot, cap: cap, okuriPrefix: okuriPrefix,
            parts: &parts, splits: &splits, enumOrder: &enumOrder
        )
        return splits
    }

    private static func enumerateRecursive(
        k: Int, depth: Int, start: Int,
        chars: [Character], snapshot: DictionarySnapshot, cap: Int,
        okuriPrefix: Character?,
        parts: inout [String], splits: inout [SplitInfo], enumOrder: inout Int
    ) {
        let n = chars.count
        if depth == k {
            if start == n {
                if let info = makeSplit(
                    parts: parts,
                    snapshot: snapshot,
                    cap: cap,
                    enumOrder: enumOrder,
                    lastIsOkuriAri: okuriPrefix != nil
                ) {
                    splits.append(info)
                    enumOrder += 1
                }
            }
            return
        }
        // Each remaining part needs at least one character of yomi left.
        let remainingParts = k - depth
        if (n - start) < remainingParts { return }

        // Only the final part may consume the okuri-ari bucket; intermediate parts
        // must come from the okuri-nashi bucket per spec.
        let isLast = (depth == k - 1)
        let matches: [(length: Int, reading: String)]
        if isLast, let okuriPrefix {
            matches = snapshot.okuriAriPrefixMatches(in: chars, from: start, okuriPrefix: okuriPrefix)
        } else {
            matches = snapshot.prefixMatches(in: chars, from: start)
        }

        for match in matches {
            let nextStart = start + match.length
            if isLast {
                // The final part must consume the entire body. (For okuri-nashi
                // mode the depth == k terminal check also enforces this, but
                // making it explicit keeps the okuri-ari branch tidy.)
                if nextStart != n { continue }
            } else {
                if (n - nextStart) < (remainingParts - 1) { continue }
            }
            parts.append(match.reading)
            enumerateRecursive(
                k: k, depth: depth + 1, start: nextStart,
                chars: chars, snapshot: snapshot, cap: cap,
                okuriPrefix: okuriPrefix,
                parts: &parts, splits: &splits, enumOrder: &enumOrder
            )
            parts.removeLast()
        }
    }

    private struct SplitInfo {
        let partCandidates: [[Candidate]]
        let numParts: Int
        let minPartLen: Int
        let enumOrder: Int
    }

    private struct PQEntry {
        let splitIdx: Int
        let rank: Int
        let indices: [Int]
        let text: String
    }

    private static func makeSplit(
        parts: [String],
        snapshot: DictionarySnapshot,
        cap: Int,
        enumOrder: Int,
        lastIsOkuriAri: Bool
    ) -> SplitInfo? {
        // For okuri-ari mode the last part's dictionary key embeds the trailing
        // ASCII letter (e.g. `なs`), but the input only contributes the hiragana
        // stem. Score the split by input-consumed length so it competes fairly
        // against okuri-nashi splits of the same body.
        var partLens = parts.map { $0.count }
        if lastIsOkuriAri, !partLens.isEmpty {
            partLens[partLens.count - 1] = max(0, partLens[partLens.count - 1] - 1)
        }
        guard let minPartLen = partLens.min() else { return nil }
        let partCandidates: [[Candidate]] = parts.enumerated().map { (i, reading) in
            let isLastOkuriAri = lastIsOkuriAri && (i == parts.count - 1)
            let all: [Candidate]
            if isLastOkuriAri {
                all = snapshot.okuriAriCandidates(for: reading)
            } else {
                all = snapshot.candidates(for: reading)
            }
            if all.count <= cap { return all }
            return Array(all.prefix(cap))
        }
        if partCandidates.contains(where: { $0.isEmpty }) { return nil }
        return SplitInfo(
            partCandidates: partCandidates,
            numParts: parts.count,
            minPartLen: minPartLen,
            enumOrder: enumOrder
        )
    }

    private static func buildText(info: SplitInfo, indices: [Int]) -> String {
        var text = ""
        for (partIdx, candIdx) in indices.enumerated() {
            text.append(info.partCandidates[partIdx][candIdx].text)
        }
        return text
    }

    private static func compareSplitKey(_ a: SplitInfo, _ b: SplitInfo) -> Int {
        // Within a single expand() call all splits share numParts; cross-k ordering
        // is enforced by the outer k loop in generate(). The only hierarchical
        // within-k signal is minPartLen (longer shortest part wins); enumOrder is
        // the deterministic fallback. Round-robin between splits that tie on these
        // keys is enforced by the heap comparator, not here.
        if a.minPartLen != b.minPartLen { return a.minPartLen > b.minPartLen ? -1 : 1 }
        if a.enumOrder != b.enumOrder { return a.enumOrder < b.enumOrder ? -1 : 1 }
        return 0
    }
}

private struct MinHeap<Element> {
    private var items: [Element] = []
    private let isLess: (Element, Element) -> Bool

    init(isLess: @escaping (Element, Element) -> Bool) {
        self.isLess = isLess
    }

    mutating func push(_ item: Element) {
        items.append(item)
        siftUp(items.count - 1)
    }

    mutating func pop() -> Element? {
        guard !items.isEmpty else { return nil }
        let result = items[0]
        if items.count > 1 {
            items[0] = items.removeLast()
            siftDown(0)
        } else {
            items.removeLast()
        }
        return result
    }

    private mutating func siftUp(_ index: Int) {
        var idx = index
        while idx > 0 {
            let parent = (idx - 1) / 2
            if isLess(items[idx], items[parent]) {
                items.swapAt(idx, parent)
                idx = parent
            } else { break }
        }
    }

    private mutating func siftDown(_ index: Int) {
        var idx = index
        let n = items.count
        while true {
            let left = idx * 2 + 1
            let right = idx * 2 + 2
            var best = idx
            if left < n, isLess(items[left], items[best]) { best = left }
            if right < n, isLess(items[right], items[best]) { best = right }
            if best == idx { break }
            items.swapAt(idx, best)
            idx = best
        }
    }
}
