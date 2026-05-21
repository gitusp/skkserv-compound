// SPDX-License-Identifier: MIT

import Foundation
import NIOCore
import NIOPosix
import Logging

public struct SKKServer: Sendable {
    public let version: String
    public let serverName: String
    private let logger: Logger
    private let store: DictionaryStore
    private let generatorConfig: CompoundGeneratorConfig
    private let allocator = ByteBufferAllocator()

    public init(
        version: String,
        serverName: String = "skkserv-compound",
        logger: Logger,
        store: DictionaryStore,
        generatorConfig: CompoundGeneratorConfig = CompoundGeneratorConfig()
    ) {
        self.version = version
        self.serverName = serverName
        self.logger = logger
        self.store = store
        self.generatorConfig = generatorConfig
    }

    public func run(
        host: String = "127.0.0.1",
        port: Int = 1178,
        incomingCharset: String.Encoding = .utf8
    ) async throws {
        let server = try await ServerBootstrap(group: NIOSingletons.posixEventLoopGroup)
            .serverChannelOption(.socketOption(.so_reuseaddr), value: 1)
            .bind(host: host, port: port) { channel in
                channel.eventLoop.makeCompletedFuture {
                    return try NIOAsyncChannel(
                        wrappingChannelSynchronously: channel,
                        configuration: NIOAsyncChannel.Configuration(
                            inboundType: ByteBuffer.self,
                            outboundType: ByteBuffer.self
                        )
                    )
                }
            }
        logger.notice("Server started on \(host):\(port) with incoming charset \(incomingCharset.rawValue).")

        try await withThrowingDiscardingTaskGroup { group in
            try await server.executeThenClose { clients in
                for try await client in clients {
                    group.addTask {
                        await handleClient(client: client, host: host, port: port, incomingCharset: incomingCharset)
                    }
                }
            }
        }
    }

    func handleClient(
        client: NIOAsyncChannel<ByteBuffer, ByteBuffer>,
        host: String,
        port: Int,
        incomingCharset: String.Encoding
    ) async {
        do {
            try await client.executeThenClose { inbound, outbound in
                var pending = allocator.buffer(capacity: 256)
                outer: for try await var message in inbound {
                    pending.writeBuffer(&message)
                    let requests = Self.extractMessages(
                        buffer: &pending,
                        charset: incomingCharset,
                        logger: logger
                    )
                    for request in requests {
                        switch await handleOpcode(
                            request.opcode,
                            operand: request.operand,
                            host: host,
                            port: port
                        ) {
                        case .close:
                            break outer
                        case .ignore:
                            continue
                        case .reply(let body):
                            try await outbound.write(allocator.buffer(string: body))
                        }
                    }
                }
            }
            logger.notice("Connection closed")
        } catch {
            logger.warning("Hit error: \(error)")
        }
    }

    /// Slice `buffer` into individual skkserv requests at space (0x20) or LF (0x0A) boundaries.
    /// Unterminated trailing bytes are left in `buffer` for the next read.
    static func extractMessages(
        buffer: inout ByteBuffer,
        charset: String.Encoding,
        logger: Logger? = nil
    ) -> [(opcode: Character, operand: String)] {
        var results: [(opcode: Character, operand: String)] = []
        while let payloadLength = nextDelimiterOffset(in: buffer) {
            let payload = buffer.readBytes(length: payloadLength) ?? []
            buffer.moveReaderIndex(forwardBy: 1) // consume delimiter
            if payload.isEmpty { continue }
            guard let text = String(bytes: payload, encoding: charset),
                  let opcode = text.first else {
                logger?.warning("Failed to decode \(payload.count)-byte request; skipping")
                continue
            }
            results.append((opcode, String(text.dropFirst())))
        }
        buffer.discardReadBytes()
        return results
    }

    private static func nextDelimiterOffset(in buffer: ByteBuffer) -> Int? {
        let view = buffer.readableBytesView
        guard let absoluteIndex = view.firstIndex(where: { $0 == 0x20 || $0 == 0x0A }) else {
            return nil
        }
        return absoluteIndex - view.startIndex
    }

    enum OpcodeResult: Equatable {
        case close
        case ignore
        case reply(String)
    }

    func handleOpcode(_ opcode: Character, operand: String, host: String, port: Int) async -> OpcodeResult {
        switch opcode {
        case "0":
            return .close
        case "1":
            return .reply(await candidateResponse(for: operand))
        case "2":
            return .reply("\(serverName)/\(version) ")
        case "3":
            let hostname = Host.current().localizedName ?? ""
            return .reply("\(hostname)/\(host):\(port) ")
        case "4":
            return .reply("4\n")
        default:
            logger.warning("Unsupported opcode: \(opcode)")
            return .ignore
        }
    }

    func candidateResponse(for rawYomi: String) async -> String {
        let (body, okuriPrefix) = sanitizeYomi(rawYomi)
        if body.isEmpty { return "4\n" }
        let snapshot = await store.current()
        let candidates = CompoundGenerator.generate(
            yomi: body,
            snapshot: snapshot,
            config: generatorConfig,
            okuriPrefix: okuriPrefix
        )
        if candidates.isEmpty {
            return "4\n"
        }
        return "1/" + candidates.joined(separator: "/") + "/\n"
    }
}

/// Normalize a raw skkserv yomi into a `(body, okuriPrefix)` pair. Trims
/// whitespace, lifts a trailing `<hiragana><a-z>` letter into `okuriPrefix`,
/// and passes all-ASCII (abbrev) inputs through verbatim.
func sanitizeYomi(_ yomi: String) -> (body: String, okuriPrefix: String?) {
    let trimmed = yomi.trimmingCharacters(in: .whitespacesAndNewlines)
    if trimmed.isEmpty { return ("", nil) }
    if trimmed.unicodeScalars.allSatisfy({ $0.isASCII }) {
        return (trimmed, nil)
    }
    if let okuri = DictionaryParser.trailingOkuri(of: trimmed) {
        return (String(trimmed.dropLast()), String(okuri))
    }
    return (trimmed, nil)
}
