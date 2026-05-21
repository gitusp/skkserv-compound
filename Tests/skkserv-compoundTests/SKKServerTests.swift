// SPDX-License-Identifier: MIT

import Testing
import Foundation
import Logging
import NIOCore
@testable import skkserv_compound

@Suite("SKKServer response")
struct SKKServerResponseTests {
    private static let testLogger = Logger(label: "server-test")

    private func makeServer(
        user: [(String, [String])] = [],
        system: [(String, [String])] = [],
        config: CompoundGeneratorConfig = CompoundGeneratorConfig()
    ) -> SKKServer {
        let snapshot = DictionaryLoader.buildSnapshot(
            user: user.map { ParsedEntry(reading: $0.0, candidates: $0.1) },
            system: system.map { ParsedEntry(reading: $0.0, candidates: $0.1) }
        )
        let store = DictionaryStore(initial: snapshot)
        return SKKServer(
            version: "test",
            logger: Self.testLogger,
            store: store,
            generatorConfig: config
        )
    }

    @Test("opcode 1: 候補ありで 1/.../\\n を返す")
    func opcode1ReturnsCandidates() async {
        let server = makeServer(system: [
            ("せいそう", ["清掃"]),
            ("ぎょうしゃ", ["業者"])
        ])
        let response = await server.candidateResponse(for: "せいそうぎょうしゃ ")
        #expect(response == "1/清掃業者/\n")
    }

    @Test("opcode 1: 候補なしで 4\\n を返す")
    func opcode1ReturnsFourOnMiss() async {
        let server = makeServer()
        let response = await server.candidateResponse(for: "そんざいしない ")
        #expect(response == "4\n")
    }

    @Test("入力読みの前後空白・改行を除去する")
    func trimsWhitespace() async {
        let server = makeServer(system: [
            ("あ", ["亜"]),
            ("い", ["胃"])
        ])
        let response = await server.candidateResponse(for: "  あい\n")
        #expect(response == "1/亜胃/\n")
    }

    @Test("末尾の送り仮名マーカーを除去する (送りあり辞書未登録なら okuri-ari 連結は出ない)")
    func stripsOkuriMarker() async {
        // okuri-ari 辞書に該当エントリがない場合、 okuriPrefix 付きリクエストの応答は
        // 送りあり連結のみで構成されるため空 (送りなし結果は混ぜない)。
        let server = makeServer(system: [
            ("あ", ["亜"]),
            ("い", ["胃"])
        ])
        let response = await server.candidateResponse(for: "あいi")
        #expect(response == "4\n")
    }

    @Test("opcode 1: もんだいなs で 1/問題無/問題済/\\n を返す")
    func opcode1OkuriAriCompound() async {
        let user = [
            ParsedEntry(reading: "もんだい", candidates: ["問題"], isOkuriAri: false),
            ParsedEntry(reading: "なs", candidates: ["無", "済"], isOkuriAri: true)
        ]
        let snapshot = DictionaryLoader.buildSnapshot(user: user, system: [])
        let server = SKKServer(
            version: "test",
            logger: Self.testLogger,
            store: DictionaryStore(initial: snapshot)
        )
        let response = await server.candidateResponse(for: "もんだいなs ")
        #expect(response == "1/問題無/問題済/\n")
    }

    @Test("opcode 1: 全 ASCII 入力 kawaii では okuriPrefix を抽出せず abbrev 連結を引く")
    func opcode1AsciiInputDoesNotExtractOkuri() async {
        let server = makeServer(system: [
            ("ka", ["カ"]),
            ("waii", ["ワイイ"])
        ])
        let response = await server.candidateResponse(for: "kawaii ")
        // sanitize は okuriPrefix を抽出せず body=`kawaii` のまま abbrev 連結探索する。
        #expect(response == "1/カワイイ/\n")
    }
}

@Suite("SKKServer opcodes")
struct SKKServerOpcodeTests {
    private static let testLogger = Logger(label: "server-opcode-test")

    private func makeServer(
        user: [(String, [String])] = [],
        system: [(String, [String])] = []
    ) -> SKKServer {
        let snapshot = DictionaryLoader.buildSnapshot(
            user: user.map { ParsedEntry(reading: $0.0, candidates: $0.1) },
            system: system.map { ParsedEntry(reading: $0.0, candidates: $0.1) }
        )
        return SKKServer(
            version: "v1",
            serverName: "skkserv-test",
            logger: Self.testLogger,
            store: DictionaryStore(initial: snapshot)
        )
    }

    @Test("opcode 0 はコネクションを閉じる")
    func opcodeZeroCloses() async {
        let server = makeServer()
        #expect(await server.handleOpcode("0", operand: "", host: "127.0.0.1", port: 1178) == .close)
    }

    @Test("opcode 2 で version 応答を返す")
    func opcodeTwoReturnsVersion() async {
        let server = makeServer()
        #expect(await server.handleOpcode("2", operand: "", host: "127.0.0.1", port: 1178) == .reply("skkserv-test/v1 "))
    }

    @Test("opcode 3 で host/port 応答を返す")
    func opcodeThreeReturnsHostPort() async {
        let server = makeServer()
        let result = await server.handleOpcode("3", operand: "", host: "127.0.0.1", port: 1178)
        guard case .reply(let body) = result else {
            Issue.record("expected reply, got \(result)")
            return
        }
        #expect(body.hasSuffix("/127.0.0.1:1178 "))
    }

    @Test("opcode 4 は常に 4\\n を返す")
    func opcodeFourReturnsFour() async {
        let server = makeServer()
        #expect(await server.handleOpcode("4", operand: "なにか ", host: "127.0.0.1", port: 1178) == .reply("4\n"))
    }

    @Test("opcode 1 はパイプライン経由でも 1/.../\\n を返す")
    func opcodeOneIntegrates() async {
        let server = makeServer(system: [
            ("あ", ["亜"]),
            ("い", ["胃"])
        ])
        #expect(await server.handleOpcode("1", operand: "あい ", host: "127.0.0.1", port: 1178) == .reply("1/亜胃/\n"))
    }

    @Test("未対応 opcode は ignore")
    func unsupportedOpcodeIgnored() async {
        let server = makeServer()
        #expect(await server.handleOpcode("9", operand: "", host: "127.0.0.1", port: 1178) == .ignore)
    }
}

@Suite("Incoming charset")
struct IncomingCharsetTests {
    @Test("EUC-JP のバイト列を読みとして復号できる")
    func decodesEucJp() {
        // "1あい " encoded as EUC-JP
        let text = "1あい "
        guard let bytes = text.data(using: .japaneseEUC) else {
            Issue.record("EUC-JP encoding failed")
            return
        }
        let roundTrip = String(data: bytes, encoding: .japaneseEUC)
        #expect(roundTrip == text)
        // Sanitize the operand portion the way the server does.
        let operand = String(text.dropFirst())
        let (body, okuri) = sanitizeYomi(operand)
        #expect(body == "あい")
        #expect(okuri == nil)
    }
}

@Suite("SKKServer.extractMessages")
struct ExtractMessagesTests {
    private static let testLogger = Logger(label: "extract-test")

    private func makeBuffer(_ bytes: [UInt8]) -> ByteBuffer {
        var buffer = ByteBufferAllocator().buffer(capacity: bytes.count)
        buffer.writeBytes(bytes)
        return buffer
    }

    private func makeBuffer(_ string: String, encoding: String.Encoding = .utf8) -> ByteBuffer {
        let data = string.data(using: encoding) ?? Data()
        return makeBuffer(Array(data))
    }

    @Test("1 つの ByteBuffer に複数のリクエストが詰まっていても全て切り出せる")
    func extractsMultipleRequestsFromSingleBuffer() {
        var buffer = makeBuffer("1あい 1うえ ")
        let messages = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(messages.count == 2)
        #expect(messages[0].opcode == "1")
        #expect(messages[0].operand == "あい")
        #expect(messages[1].opcode == "1")
        #expect(messages[1].operand == "うえ")
        #expect(buffer.readableBytes == 0)
    }

    @Test("リクエストが複数の ByteBuffer に分断されて届いても結合できる")
    func reassemblesAcrossBuffers() {
        let full = "1あい "
        let fullBytes = Array(full.data(using: .utf8)!)
        let splitAt = fullBytes.count / 2
        var buffer = makeBuffer(Array(fullBytes[..<splitAt]))

        let firstPass = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(firstPass.isEmpty)
        #expect(buffer.readableBytes > 0)

        var second = makeBuffer(Array(fullBytes[splitAt...]))
        buffer.writeBuffer(&second)
        let secondPass = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(secondPass.count == 1)
        #expect(secondPass[0].opcode == "1")
        #expect(secondPass[0].operand == "あい")
        #expect(buffer.readableBytes == 0)
    }

    @Test("LF 区切りも space 区切りも両方認識する")
    func recognizesBothDelimiters() {
        var buffer = makeBuffer("1あ\n1い ")
        let messages = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(messages.count == 2)
        #expect(messages[0].opcode == "1")
        #expect(messages[0].operand == "あ")
        #expect(messages[1].opcode == "1")
        #expect(messages[1].operand == "い")
    }

    @Test("区切り未到達の末尾はバッファに残す")
    func keepsUnterminatedTail() {
        var buffer = makeBuffer("1あい 1うえ")
        let messages = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(messages.count == 1)
        #expect(messages[0].operand == "あい")
        #expect(buffer.readableBytes > 0)
    }

    @Test("EUC-JP のバイト列も同じ境界規則で切り出せる")
    func extractsEucJpRequests() {
        var buffer = makeBuffer("1あい 1うえ ", encoding: .japaneseEUC)
        let messages = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .japaneseEUC,
            logger: Self.testLogger
        )
        #expect(messages.count == 2)
        #expect(messages[0].operand == "あい")
        #expect(messages[1].operand == "うえ")
    }

    @Test("空ペイロード (連続区切り) はスキップする")
    func skipsEmptyPayloads() {
        var buffer = makeBuffer("  1あ ")
        let messages = SKKServer.extractMessages(
            buffer: &buffer,
            charset: .utf8,
            logger: Self.testLogger
        )
        #expect(messages.count == 1)
        #expect(messages[0].opcode == "1")
        #expect(messages[0].operand == "あ")
    }
}

@Suite("sanitizeYomi")
struct SanitizeYomiTests {
    @Test("trim する; okuriPrefix なし")
    func trimsAndStrips() {
        let a = sanitizeYomi(" あい \n")
        #expect(a.body == "あい")
        #expect(a.okuriPrefix == nil)

        // 全 ASCII 入力は abbrev とみなし okuriPrefix を抽出しない
        let abc = sanitizeYomi("abc")
        #expect(abc.body == "abc")
        #expect(abc.okuriPrefix == nil)
    }

    @Test("ひらがな + ASCII 1 文字なら okuriPrefix として保持する")
    func extractsOkuriPrefix() {
        let s = sanitizeYomi("おくr")
        #expect(s.body == "おく")
        #expect(s.okuriPrefix == "r")

        let m = sanitizeYomi("もんだいなs ")
        #expect(m.body == "もんだいな")
        #expect(m.okuriPrefix == "s")
    }

    @Test("空入力は (\"\", nil) を返す")
    func returnsEmptyTuple() {
        let s = sanitizeYomi("   \n")
        #expect(s.body == "")
        #expect(s.okuriPrefix == nil)
    }
}
