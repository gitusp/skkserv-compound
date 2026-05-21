// SPDX-License-Identifier: MIT

import Testing
import Foundation
@testable import skkserv_compound

@Suite("DictionaryLoader merge")
struct DictionaryLoaderMergeTests {
    @Test("user と system をマージできる")
    func mergesUserAndSystem() {
        let user = [ParsedEntry(reading: "か", candidates: ["蚊"])]
        let system = [
            ParsedEntry(reading: "か", candidates: ["化"]),
            ParsedEntry(reading: "せいそう", candidates: ["清掃"])
        ]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: system)
        let kaCandidates = snapshot.candidates(for: "か")
        #expect(kaCandidates.map(\.text) == ["蚊", "化"])
        #expect(snapshot.candidates(for: "せいそう").map(\.text) == ["清掃"])
    }

    @Test("同じ読み・同じ候補は user 由来として扱う")
    func sameCandidatePrefersUserSource() {
        let user = [ParsedEntry(reading: "か", candidates: ["化"])]
        let system = [ParsedEntry(reading: "か", candidates: ["化", "蚊"])]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: system)
        let cs = snapshot.candidates(for: "か")
        #expect(cs.count == 2)
        #expect(cs[0].text == "化")
        #expect(cs[0].source == .user)
        #expect(cs[1].text == "蚊")
        #expect(cs[1].source == .system)
    }

    @Test("同じ読み内で user 候補を system 候補より前に置く")
    func userCandidatesPrecedeSystemCandidates() {
        let user = [ParsedEntry(reading: "あ", candidates: ["亜"])]
        let system = [ParsedEntry(reading: "あ", candidates: ["阿", "唖"])]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: system)
        let cs = snapshot.candidates(for: "あ").map(\.text)
        #expect(cs == ["亜", "阿", "唖"])
    }

    @Test("送りあり entry は okuriAri バケットに入る")
    func okuriAriEntriesGoIntoOkuriAriBucket() {
        let user = [ParsedEntry(reading: "なs", candidates: ["無"], isOkuriAri: true)]
        let system = [
            ParsedEntry(reading: "なs", candidates: ["済"], isOkuriAri: true),
            ParsedEntry(reading: "もんだい", candidates: ["問題"])
        ]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: system)
        // okuri-ari bucket holds なs and merges user > system.
        let okuriAri = snapshot.okuriAriCandidates(for: "なs")
        #expect(okuriAri.map(\.text) == ["無", "済"])
        #expect(okuriAri[0].source == .user)
        #expect(okuriAri[1].source == .system)
        // okuri-nashi bucket only holds the unflagged entry.
        #expect(snapshot.candidates(for: "なs").isEmpty)
        #expect(snapshot.candidates(for: "もんだい").map(\.text) == ["問題"])
        // okuri-ari readings are not visible from the okuri-nashi first-char index.
        #expect(snapshot.readings(startingWith: "な").isEmpty)
    }

    @Test("送りなし entry は通常バケットに入り続ける (regression)")
    func okuriNashiEntriesStayInDefaultBucket() {
        let user = [ParsedEntry(reading: "あ", candidates: ["亜"])]
        let system = [ParsedEntry(reading: "あ", candidates: ["阿"])]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: system)
        #expect(snapshot.candidates(for: "あ").map(\.text) == ["亜", "阿"])
        #expect(snapshot.okuriAriCandidates(for: "あ").isEmpty)
    }
}

@Suite("DictionaryLoader multiple system dictionaries")
struct DictionaryLoaderMultiSystemTests {
    private func makeTempDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("skkserv-compound-loader-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private func writeFile(_ contents: String, at url: URL) throws {
        try contents.write(to: url, atomically: true, encoding: .utf8)
    }

    @Test("複数のシステム辞書を CLI 指定順で優先する")
    func prefersFirstListedSystemDictionary() throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }

        let userURL = dir.appendingPathComponent("user.dict")
        let system1URL = dir.appendingPathComponent("system1.dict")
        let system2URL = dir.appendingPathComponent("system2.dict")
        try writeFile("", at: userURL)
        try writeFile("か /化/\n", at: system1URL)
        try writeFile("か /課/\n", at: system2URL)

        let snapshot = try DictionaryLoader.loadSnapshot(
            userDictionaryPath: userURL.path,
            systemDictionaryPaths: [system1URL.path, system2URL.path]
        )

        let cs = snapshot.candidates(for: "か")
        #expect(cs.map(\.text) == ["化", "課"])
        #expect(cs.allSatisfy { $0.source == .system })
    }

    @Test("複数 system の重複候補は先指定 system 由来になる")
    func duplicateCandidateGoesToFirstSystem() throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }

        let userURL = dir.appendingPathComponent("user.dict")
        let system1URL = dir.appendingPathComponent("system1.dict")
        let system2URL = dir.appendingPathComponent("system2.dict")
        try writeFile("", at: userURL)
        try writeFile("か /化/蚊/\n", at: system1URL)
        try writeFile("か /課/化/\n", at: system2URL)

        let snapshot = try DictionaryLoader.loadSnapshot(
            userDictionaryPath: userURL.path,
            systemDictionaryPaths: [system1URL.path, system2URL.path]
        )

        let cs = snapshot.candidates(for: "か")
        // first-listed system 由来の "化","蚊" がまず並び、後続 system からは新規の "課" だけが加わる。
        #expect(cs.map(\.text) == ["化", "蚊", "課"])
        #expect(cs.allSatisfy { $0.source == .system })
    }

    @Test("複数 system 辞書での okuriAri マージ")
    func mergesOkuriAriAcrossSystemDictionaries() throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }

        let userURL = dir.appendingPathComponent("user.dict")
        let system1URL = dir.appendingPathComponent("system1.dict")
        let system2URL = dir.appendingPathComponent("system2.dict")
        try writeFile("なs /済/\n", at: userURL)
        try writeFile("なs /無/済/\n", at: system1URL)
        try writeFile("なs /為/\n", at: system2URL)

        let snapshot = try DictionaryLoader.loadSnapshot(
            userDictionaryPath: userURL.path,
            systemDictionaryPaths: [system1URL.path, system2URL.path]
        )

        let cs = snapshot.okuriAriCandidates(for: "なs")
        // user の `済` が先頭、 続いて system1 由来の新規 `無`、 最後に system2 由来の `為`。
        #expect(cs.map(\.text) == ["済", "無", "為"])
        #expect(cs[0].source == .user)
        #expect(cs[1].source == .system)
        #expect(cs[2].source == .system)
    }

    @Test("system 辞書が 0 個でも user 辞書だけでロードできる")
    func loadsWithoutSystemDictionaries() throws {
        let dir = try makeTempDir()
        defer { try? FileManager.default.removeItem(at: dir) }

        let userURL = dir.appendingPathComponent("user.dict")
        try writeFile("あ /亜/\n", at: userURL)

        let snapshot = try DictionaryLoader.loadSnapshot(
            userDictionaryPath: userURL.path,
            systemDictionaryPaths: []
        )

        #expect(snapshot.candidates(for: "あ").map(\.text) == ["亜"])
    }
}

@Suite("DictionarySnapshot index")
struct DictionarySnapshotIndexTests {
    @Test("prefix 検索で該当する読みを引ける")
    func prefixSearch() {
        let snapshot = DictionaryLoader.buildSnapshot(
            user: [],
            system: [
                ParsedEntry(reading: "せいそう", candidates: ["清掃"]),
                ParsedEntry(reading: "ぎょうしゃ", candidates: ["業者"])
            ]
        )
        let chars = Array("せいそうぎょうしゃ")
        let matches = snapshot.prefixMatches(in: chars, from: 0)
        #expect(matches.map(\.reading) == ["せいそう"])
    }

    @Test("読みの先頭文字から候補読み集合を絞り込める")
    func firstCharacterIndex() {
        let snapshot = DictionaryLoader.buildSnapshot(
            user: [],
            system: [
                ParsedEntry(reading: "か", candidates: ["化"]),
                ParsedEntry(reading: "かわ", candidates: ["川"]),
                ParsedEntry(reading: "き", candidates: ["木"])
            ]
        )
        let readings = snapshot.readings(startingWith: "か").sorted()
        #expect(readings == ["か", "かわ"])
        #expect(snapshot.readings(startingWith: "き") == ["き"])
    }
}
