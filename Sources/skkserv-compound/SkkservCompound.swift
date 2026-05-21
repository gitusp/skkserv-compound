// SPDX-License-Identifier: MIT

import Foundation
import ArgumentParser
import Logging

let version = "0.1.0"

@main
struct SkkservCompound: ParsableCommand {
    static let configuration = CommandConfiguration(
        commandName: "skkserv-compound",
        abstract: "A skkserv that returns compound candidates built from SKK dictionaries.",
        version: version
    )

    @Option(help: "Network address to bind to.")
    var bindAddress: String = "127.0.0.1"

    @Option(help: "The network port number to use.")
    var port: Int = 1178

    @Option(help: "The expected incoming character set.")
    var incomingCharset: IncomingCharset = .utf8

    @Option(help: "Path to the SKK user dictionary file (required).")
    var userDictionary: String

    @Option(name: .customLong("system-dictionary"), help: "Path to an SKK system dictionary file. Pass multiple times to merge several system dictionaries; earlier occurrences win on conflicts.")
    var systemDictionaries: [String] = []

    @Option(help: "Maximum number of candidates pulled from each reading part.")
    var maxCandidatesPerReading: Int = 5

    @Option(help: "Maximum number of final compound candidates returned.")
    var maxFinalCandidates: Int = 10

    @Option(help: "Log level (trace|debug|info|notice|warning|error|critical).")
    var logLevel: LogLevel = .notice

    func run() throws {
        LoggingSystem.bootstrap(StreamLogHandler.standardError)
        var logger = Logger(label: "io.github.gitusp.skkserv-compound")
        logger.logLevel = logLevel.loggerLevel

        let store = DictionaryStore()
        let watcher = UserDictionaryWatcher(
            userDictionaryPath: userDictionary,
            systemDictionaryPaths: systemDictionaries,
            store: store,
            logger: logger
        )

        let server = SKKServer(
            version: version,
            logger: logger,
            store: store,
            generatorConfig: CompoundGeneratorConfig(
                maxCandidatesPerReading: maxCandidatesPerReading,
                maxFinalCandidates: maxFinalCandidates
            )
        )

        Task {
            do {
                try await watcher.start()
                try await server.run(host: bindAddress, port: port, incomingCharset: incomingCharset.stringEncoding)
            } catch {
                logger.error("An error occurred: \(error)")
                abort()
            }
        }

        dispatchMain()
    }
}

extension ExpressibleByArgument where Self: CaseIterable & RawRepresentable, RawValue == String {
    static var defaultCompletionKind: CompletionKind {
        .list(allCases.map(\.rawValue))
    }
}

enum IncomingCharset: String, ExpressibleByArgument, CaseIterable {
    case utf8 = "UTF-8"
    case eucjp = "EUC-JP"

    var stringEncoding: String.Encoding {
        switch self {
        case .utf8: return .utf8
        case .eucjp: return .japaneseEUC
        }
    }
}

enum LogLevel: String, ExpressibleByArgument, CaseIterable {
    case trace
    case debug
    case info
    case notice
    case warning
    case error
    case critical

    var loggerLevel: Logger.Level {
        switch self {
        case .trace: return .trace
        case .debug: return .debug
        case .info: return .info
        case .notice: return .notice
        case .warning: return .warning
        case .error: return .error
        case .critical: return .critical
        }
    }
}
