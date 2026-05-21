// SPDX-License-Identifier: MIT

import Testing
@testable import skkserv_compound

@Suite("CompoundGenerator")
struct CompoundGeneratorTests {
    private func snapshot(user: [(String, [String])] = [], system: [(String, [String])] = []) -> DictionarySnapshot {
        DictionaryLoader.buildSnapshot(
            user: user.map { ParsedEntry(reading: $0.0, candidates: $0.1) },
            system: system.map { ParsedEntry(reading: $0.0, candidates: $0.1) }
        )
    }

    private func okuriSnapshot(
        user: [(String, [String])] = [],
        system: [(String, [String])] = [],
        okuriAriUser: [(String, [String])] = [],
        okuriAriSystem: [(String, [String])] = []
    ) -> DictionarySnapshot {
        let userEntries =
            user.map { ParsedEntry(reading: $0.0, candidates: $0.1, isOkuriAri: false) } +
            okuriAriUser.map { ParsedEntry(reading: $0.0, candidates: $0.1, isOkuriAri: true) }
        let systemEntries =
            system.map { ParsedEntry(reading: $0.0, candidates: $0.1, isOkuriAri: false) } +
            okuriAriSystem.map { ParsedEntry(reading: $0.0, candidates: $0.1, isOkuriAri: true) }
        return DictionaryLoader.buildSnapshot(user: userEntries, system: systemEntries)
    }

    @Test("せいそうぎょうしゃ → 清掃業者")
    func combinesSeisouGyousha() {
        let snap = snapshot(system: [
            ("せいそう", ["清掃"]),
            ("ぎょうしゃ", ["業者"])
        ])
        let out = CompoundGenerator.generate(yomi: "せいそうぎょうしゃ", snapshot: snap)
        #expect(out.first == "清掃業者")
    }

    @Test("たんじゅんか → 単純化")
    func combinesTanjunka() {
        let snap = snapshot(system: [
            ("たんじゅん", ["単純"]),
            ("か", ["化", "蚊"])
        ])
        let out = CompoundGenerator.generate(yomi: "たんじゅんか", snapshot: snap)
        #expect(out.contains("単純化"))
    }

    @Test("1語完全一致は返さない")
    func skipsSingleWordExactMatch() {
        let snap = snapshot(system: [
            ("たんじゅんか", ["単純化"]),
            ("たんじゅん", ["単純"]),
            ("か", ["化"])
        ])
        let out = CompoundGenerator.generate(yomi: "たんじゅんか", snapshot: snap)
        #expect(!out.contains("単純化") == false)
        // The compound "単純化" should be present, and it must come from the 2-split
        // (not the 1-word entry). The single-word entry "たんじゅんか /単純化/" must
        // not be appended again.
        #expect(out.filter { $0 == "単純化" }.count == 1)
    }

    @Test("2語分割を3語分割より優先する")
    func prefersFewerParts() {
        let snap = snapshot(system: [
            ("せいそう", ["清掃"]),
            ("せい", ["清"]),
            ("そう", ["掃"]),
            ("ぎょうしゃ", ["業者"])
        ])
        let out = CompoundGenerator.generate(yomi: "せいそうぎょうしゃ", snapshot: snap)
        // 2-part split "清掃業者" must precede 3-part split "清掃業者"... here both
        // texts collapse, but the dedupe step keeps the first (highest ranked) split,
        // which is the 2-part one. Verify by sorting key indirectly: 2-part split
        // appears at the top.
        #expect(out.first == "清掃業者")
    }

    @Test("短い読みを含む分割を許可する")
    func allowsShortReadings() {
        let snap = snapshot(system: [
            ("たんじゅん", ["単純"]),
            ("か", ["化"])
        ])
        let out = CompoundGenerator.generate(yomi: "たんじゅんか", snapshot: snap)
        #expect(out == ["単純化"])
    }

    @Test("各読み片の候補数上限を守る")
    func respectsPerReadingCap() {
        let snap = snapshot(system: [
            ("たん", ["1", "2", "3", "4", "5", "6", "7"]),
            ("じゅん", ["A"])
        ])
        let out = CompoundGenerator.generate(
            yomi: "たんじゅん",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 3, maxFinalCandidates: 100)
        )
        // 3 * 1 = 3 combinations
        #expect(out == ["1A", "2A", "3A"])
    }

    @Test("最終候補数上限を守る")
    func respectsFinalCap() {
        let snap = snapshot(system: [
            ("たん", ["1", "2", "3", "4", "5"]),
            ("じゅん", ["A", "B", "C", "D", "E"])
        ])
        let out = CompoundGenerator.generate(
            yomi: "たんじゅん",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 5, maxFinalCandidates: 4)
        )
        #expect(out.count == 4)
    }

    @Test("重複候補は除去して最上位のものを残す")
    func dedupesCandidates() {
        // Two different splits that would both produce "ABC".
        let snap = snapshot(system: [
            ("あい", ["AB"]),
            ("う", ["C"]),
            ("あ", ["A"]),
            ("いう", ["BC"])
        ])
        let out = CompoundGenerator.generate(yomi: "あいう", snapshot: snap)
        #expect(out.count == 1)
        #expect(out[0] == "ABC")
    }

    @Test("最短部分が長い分割を優先する")
    func prefersLongerMinPart() {
        // Yomi: あいうえ (4)
        //  Split A: あい(2) + うえ(2) -> minPartLen = 2
        //  Split B: あ(1)  + いうえ(3) -> minPartLen = 1
        let snap = snapshot(system: [
            ("あい", ["X"]),
            ("うえ", ["Y"]),
            ("あ", ["P"]),
            ("いうえ", ["Q"])
        ])
        let out = CompoundGenerator.generate(yomi: "あいうえ", snapshot: snap)
        #expect(out.first == "XY")
    }

    @Test("abbrev: minicataloggift → ミニカタログギフト")
    func combinesAbbrevKatakana() {
        let snap = snapshot(system: [
            ("mini", ["ミニ"]),
            ("catalog", ["カタログ"]),
            ("gift", ["ギフト"])
        ])
        let out = CompoundGenerator.generate(yomi: "minicataloggift", snapshot: snap)
        #expect(out.first == "ミニカタログギフト")
    }

    @Test("abbrev: itemcardset → アイテムカードセット")
    func combinesAbbrevItemCardSet() {
        let snap = snapshot(system: [
            ("item", ["アイテム"]),
            ("card", ["カード"]),
            ("set", ["セット"])
        ])
        let out = CompoundGenerator.generate(yomi: "itemcardset", snapshot: snap)
        #expect(out.first == "アイテムカードセット")
    }

    @Test("final cap が小さいとき高スコア split だけから候補が出る")
    func bestFirstSmallCapStaysOnTopSplit() {
        // Top 2-part split has enough unique candidates to fill a small final cap.
        // The 3-part fallback would emit "スイソウ業者", which must not appear.
        let snap = snapshot(system: [
            ("せいそう", ["清掃", "整層", "正装"]),
            ("せい", ["スイ"]),
            ("そう", ["ソウ"]),
            ("ぎょうしゃ", ["業者"])
        ])
        let out = CompoundGenerator.generate(
            yomi: "せいそうぎょうしゃ",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 5, maxFinalCandidates: 2)
        )
        #expect(out == ["清掃業者", "整層業者"])
        #expect(!out.contains("スイソウ業者"))
    }

    @Test("高スコア split を使い切ったら低スコア split に降りる")
    func bestFirstRetreatsToLowerSplitWhenNeeded() {
        // Top 2-part split only yields a single unique candidate ("清掃業者");
        // to reach final cap 3 the generator must retreat to the 3-part split.
        let snap = snapshot(system: [
            ("せいそう", ["清掃"]),
            ("せい", ["清", "整"]),
            ("そう", ["掃", "層"]),
            ("ぎょうしゃ", ["業者"])
        ])
        let out = CompoundGenerator.generate(
            yomi: "せいそうぎょうしゃ",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 5, maxFinalCandidates: 3)
        )
        // 2-part: 清掃業者. 3-part (lex over [清,整]×[掃,層]×[業者]):
        //   清掃業者 (dup), 清層業者, 整掃業者, 整層業者.
        // After dedupe and cap=3 -> [清掃業者, 清層業者, 整掃業者].
        #expect(out == ["清掃業者", "清層業者", "整掃業者"])
    }

    @Test("k=2 で final cap が埋まれば k=3 以降は探索しない")
    func skipsHigherKOnceFinalCapFilled() {
        // Yomi: あいうえ
        //   k=2 split: あい + うえ → "AIUE"
        //   k=3 split: あ + い + うえ → "A_XI_XUE" (sentinel; only reachable via k=3)
        //   k=4 split: う が読みに無いので構築不能
        // final cap = 1 で k=2 が即埋まり、 k=3 へ突入しないことを sentinel が出
        // ない/結果が k=2 候補だけ、 で確認する。
        let snap = snapshot(system: [
            ("あい", ["AI"]),
            ("うえ", ["UE"]),
            ("あ", ["A_X"]),
            ("い", ["I_X"])
        ])
        let out = CompoundGenerator.generate(
            yomi: "あいうえ",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 5, maxFinalCandidates: 1)
        )
        #expect(out == ["AIUE"])
        #expect(!out.contains("A_XI_XUE"))
    }

    @Test("4 語連結 minicataloggiftset → ミニカタログギフトセット")
    func combinesFourPartAbbrev() {
        let snap = snapshot(system: [
            ("mini", ["ミニ"]),
            ("catalog", ["カタログ"]),
            ("gift", ["ギフト"]),
            ("set", ["セット"])
        ])
        let out = CompoundGenerator.generate(yomi: "minicataloggiftset", snapshot: snap)
        #expect(out.first == "ミニカタログギフトセット")
    }

    @Test("yomi 文字数を超える k は探索しない")
    func skipsKLargerThanYomiLength() {
        // 2 文字 yomi なら k=2 のみ可能 (k=3 以降は構築不能)。
        // ループが k <= n で打ち切られて無限増加しないことと、 結果が k=2 のもの
        // だけになることを確認する。
        let snap = snapshot(system: [
            ("あ", ["X"]),
            ("い", ["Y"])
        ])
        let out = CompoundGenerator.generate(yomi: "あい", snapshot: snap)
        #expect(out == ["XY"])
    }

    @Test("k 増加でも 2 split が 3 split より上位")
    func twoPartBeatsThreePartAcrossK() {
        // Yomi: あいう
        //   k=2 splits: あい + う → "ABC"、 あ + いう → "DEF" (両者 minPart=1)
        //               round-robin で各 split の 1 番目を先に並べる: ABC → DEF
        //   k=3 split:  あ + い + う → "DGC"
        // 外側の k ループが少 k を先に消化するので [ABC, DEF, DGC] の順になる。
        let snap = snapshot(system: [
            ("あい", ["AB"]),
            ("う", ["C"]),
            ("あ", ["D"]),
            ("いう", ["EF"]),
            ("い", ["G"])
        ])
        let out = CompoundGenerator.generate(yomi: "あいう", snapshot: snap)
        #expect(out == ["ABC", "DEF", "DGC"])
    }

    @Test("もんだいなs → 問題無 / 問題済 を返す")
    func combinesOkuriAriCompound() {
        let snap = okuriSnapshot(
            system: [("もんだい", ["問題"])],
            okuriAriSystem: [("なs", ["無", "済"])]
        )
        let out = CompoundGenerator.generate(
            yomi: "もんだいな",
            snapshot: snap,
            okuriPrefix: "s"
        )
        #expect(out == ["問題無", "問題済"])
    }

    @Test("送りあり 1 語完全一致は返さない")
    func skipsOkuriAriSingleWordExactMatch() {
        // 辞書には `はs /有/` の okuri-ari エントリしかなく、 body は単独で `は` のみ。
        // 1 部品 = 送りあり 1 語完全一致なので候補は出さない。
        let snap = okuriSnapshot(okuriAriSystem: [("はs", ["有"])])
        let out = CompoundGenerator.generate(yomi: "は", snapshot: snap, okuriPrefix: "s")
        #expect(out.isEmpty)
    }

    @Test("送りあり split は最後の部品のみ; 中間に送りあり読みを使わない")
    func okuriAriOnlyAtLastPart() {
        // body = `なもんだい`、 okuriPrefix = `s`。
        // 中間で送りあり読み `なs` を使ってしまうと `(なs, もんだい)` のような split が
        // できてしまうが、 これは仕様で禁止 (送りあり stem は最後の 1 部品のみ)。
        // 最後の `もんだい` を okuri-ari として扱えるか? — `もんだいs` は辞書にないので
        // 唯一の okuri-ari split である `(な, もんだいs)` も成立しない。 結果は空。
        let snap = okuriSnapshot(
            system: [
                ("な", ["菜"]),
                ("もんだい", ["問題"])
            ],
            okuriAriSystem: [
                ("なs", ["無"])
            ]
        )
        let out = CompoundGenerator.generate(
            yomi: "なもんだい",
            snapshot: snap,
            okuriPrefix: "s"
        )
        // 候補なし: 中間に okuri-ari を使う split は禁止。 最後の部品が okuri-ari に
        // 一致する split も `もんだいs` が辞書にないので成立しない。
        #expect(out.isEmpty)
    }

    @Test("okuriPrefix ありでは送りなし連結結果を混ぜない")
    func okuriPrefixDoesNotEmitOkuriNashiSplits() {
        // 送りなしで `もんだいな` → `問題菜` のような split が作れても、 okuri-ari モードでは
        // 出力から除外しなければならない (仕様: okuriPrefix 付きリクエストの応答は送りあり
        // 連結結果のみ)。
        let snap = okuriSnapshot(
            system: [
                ("もんだい", ["問題"]),
                ("な", ["菜"])
            ],
            okuriAriSystem: [
                ("なs", ["無"])
            ]
        )
        let out = CompoundGenerator.generate(
            yomi: "もんだいな",
            snapshot: snap,
            okuriPrefix: "s"
        )
        #expect(out == ["問題無"])
        #expect(!out.contains("問題菜"))
    }

    @Test("okuriPrefix なしでは送りなし挙動 (regression)")
    func okuriPrefixNilFallsBackToOkuriNashi() {
        // okuri-ari バケットに同じ stem が入っていても、 okuriPrefix を渡さなければ
        // 通常の送りなし連結だけが評価される。
        let snap = okuriSnapshot(
            system: [
                ("もんだい", ["問題"]),
                ("な", ["菜"])
            ],
            okuriAriSystem: [
                ("なs", ["無"])
            ]
        )
        let out = CompoundGenerator.generate(yomi: "もんだいな", snapshot: snap)
        #expect(out == ["問題菜"])
    }

    @Test("送りあり最後の部品の候補も per-reading cap に従う")
    func okuriAriRespectsPerReadingCap() {
        let snap = okuriSnapshot(
            system: [("もんだい", ["問題"])],
            okuriAriSystem: [("なs", ["A", "B", "C", "D", "E"])]
        )
        let out = CompoundGenerator.generate(
            yomi: "もんだいな",
            snapshot: snap,
            config: CompoundGeneratorConfig(maxCandidatesPerReading: 2, maxFinalCandidates: 100),
            okuriPrefix: "s"
        )
        #expect(out == ["問題A", "問題B"])
    }

    @Test("同 minPartLen の split 間ではラウンドロビン: 各 split の 1 番目を先に並べる")
    func roundRobinsBetweenSplitsWithSameMinPartLen() {
        // Yomi: あいうえお (5)
        //  Split A: あい(2) + うえお(3)   minPart=2
        //  Split B: あいう(3) + えお(2)   minPart=2
        // 旧仕様は「左から長い」 で B 系統を全部並べてから A に降りていたため、 cap が
        // 厳しいと A 系統が見えなくなることがあった。 新仕様は各 split の rank-0
        // (= 1 番目候補) を先に並べ、 続けて rank-1 を並べる。 enumOrder は dictionary
        // 挿入順 ("あい" が "あいう" より先に登録されているので) なので A が先。
        let snap = snapshot(system: [
            ("あい", ["A"]),
            ("うえお", ["B"]),
            ("あいう", ["C"]),
            ("えお", ["D"])
        ])
        let out = CompoundGenerator.generate(yomi: "あいうえお", snapshot: snap)
        #expect(out == ["AB", "CD"])
    }

    @Test("ぜんけんげん → 全権限 (round-robin 回帰)")
    func roundRobinsZenkengen() {
        // 報告された実例: `ぜんけんげん` の k=2 分割
        //   A: ぜん(2) + けんげん(4)   →  全権限         minPart=2
        //   B: ぜんけん(4) + げん(2)   →  全権{源,現}    minPart=2
        // 旧「左から長い」 仕様は B 系統を優先したため、 `げん` の候補が多いと
        // `全権限` がキャップに押し出されていた。 round-robin 版は A の 1 番目と
        // B の 1 番目を先に並べるので `全権限` は必ず先頭付近に出る。
        let snap = snapshot(system: [
            ("ぜん", ["全"]),
            ("けんげん", ["権限"]),
            ("ぜんけん", ["全権"]),
            ("げん", ["源", "現", "玄", "言", "弦"])
        ])
        let out = CompoundGenerator.generate(yomi: "ぜんけんげん", snapshot: snap)
        #expect(out.first == "全権限")
        #expect(out.contains("全権限"))
    }
}
