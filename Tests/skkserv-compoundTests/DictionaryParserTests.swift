// SPDX-License-Identifier: MIT

import Testing
@testable import skkserv_compound

@Suite("DictionaryParser")
struct DictionaryParserTests {
    @Test("通常の送りなし行を読める")
    func parsesNormalEntries() {
        let entries = DictionaryParser.parse("たんじゅん /単純/\n")
        #expect(entries.count == 1)
        #expect(entries[0].reading == "たんじゅん")
        #expect(entries[0].candidates == ["単純"])
    }

    @Test("コメント行を無視する")
    func ignoresComments() {
        let source = """
        ;; okuri-nasi entries.
        ;; これはコメント
        ぎょうしゃ /業者/
        """
        let entries = DictionaryParser.parse(source)
        #expect(entries.count == 1)
        #expect(entries[0].reading == "ぎょうしゃ")
    }

    @Test("空行を無視する")
    func ignoresEmptyLines() {
        let source = "\n\nたん /担/\n\n"
        let entries = DictionaryParser.parse(source)
        #expect(entries.count == 1)
        #expect(entries[0].reading == "たん")
    }

    @Test("候補注釈を剥がす")
    func stripsAnnotations() {
        let entries = DictionaryParser.parse("か /化;接尾辞/蚊;昆虫/")
        #expect(entries.count == 1)
        #expect(entries[0].candidates == ["化", "蚊"])
    }

    @Test("不正な行は無視する")
    func ignoresMalformedLines() {
        let source = """
        broken line without slashes
        だけ /
        / 候補のみ /
        ただしい /正/
        """
        let entries = DictionaryParser.parse(source)
        #expect(entries.count == 1)
        #expect(entries[0].reading == "ただしい")
    }

    @Test("同一読みの複数候補は順序を維持する")
    func keepsCandidateOrder() {
        let entries = DictionaryParser.parse("か /化/蚊/科/")
        #expect(entries[0].candidates == ["化", "蚊", "科"])
    }

    @Test("送りありエントリは isOkuriAri: true として取り込む")
    func keepsOkuriAriWithFlag() {
        let source = """
        おくr /送/
        おくり /贈/
        """
        let entries = DictionaryParser.parse(source)
        #expect(entries.count == 2)
        #expect(entries[0].reading == "おくr")
        #expect(entries[0].candidates == ["送"])
        #expect(entries[0].isOkuriAri == true)
        #expect(entries[1].reading == "おくり")
        #expect(entries[1].isOkuriAri == false)
    }

    @Test("全 ASCII の abbrev 見出し語は isOkuriAri: false で受け入れる")
    func acceptsAbbrevEntries() {
        let source = """
        mini /ミニ/
        gift /ギフト;贈り物/
        item /アイテム/
        """
        let entries = DictionaryParser.parse(source)
        #expect(entries.map(\.reading) == ["mini", "gift", "item"])
        #expect(entries[1].candidates == ["ギフト"])
        #expect(entries.allSatisfy { $0.isOkuriAri == false })
    }

    @Test("ひらがな + ASCII 末尾 1 文字でのみ送りありと判定する")
    func okuriAriRequiresHiraganaBeforeLatin() {
        // "おくr" は送りあり扱い (isOkuriAri: true で取り込む)
        // "ABc" のような全 ASCII 連続は abbrev として通常バケットに入る
        let source = """
        おくr /送/
        ABc /Abc/
        """
        let entries = DictionaryParser.parse(source)
        #expect(entries.map(\.reading) == ["おくr", "ABc"])
        #expect(entries[0].isOkuriAri == true)
        #expect(entries[1].isOkuriAri == false)
    }

    @Test("送りなしエントリは isOkuriAri: false で取り込む")
    func okuriNashiHasFlagFalse() {
        let entries = DictionaryParser.parse("たんじゅん /単純/\nか /化/蚊/\n")
        #expect(entries.count == 2)
        #expect(entries.allSatisfy { $0.isOkuriAri == false })
    }

    @Test("行内で同じ候補が出ても1つにまとめる")
    func dedupesCandidatesWithinLine() {
        let entries = DictionaryParser.parse("か /化/化/蚊/")
        #expect(entries[0].candidates == ["化", "蚊"])
    }
}
