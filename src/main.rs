// SPDX-License-Identifier: MIT

use clap::{Parser, ValueEnum};
use skkserv_compound::generator::CompoundGeneratorConfig;
use skkserv_compound::server::{IncomingCharset, SkkServer};
use skkserv_compound::store::DictionaryStore;
use skkserv_compound::watcher::UserDictionaryWatcher;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "skkserv-compound",
    version = skkserv_compound::VERSION,
    about = "A skkserv that returns compound candidates built from SKK dictionaries."
)]
struct Args {
    /// Network address to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    bind_address: String,

    /// The network port number to use.
    #[arg(long, default_value_t = 1178)]
    port: u16,

    /// The expected incoming character set.
    #[arg(long, value_enum, default_value_t = IncomingCharsetArg::Utf8)]
    incoming_charset: IncomingCharsetArg,

    /// Path to the SKK user dictionary file (required).
    #[arg(long)]
    user_dictionary: String,

    /// Path to an SKK system dictionary file. Pass multiple times to merge
    /// several system dictionaries; earlier occurrences win on conflicts.
    #[arg(long = "system-dictionary")]
    system_dictionaries: Vec<String>,

    /// Maximum number of candidates pulled from each reading part.
    #[arg(long, default_value_t = 5)]
    max_candidates_per_reading: usize,

    /// Maximum number of final compound candidates returned.
    #[arg(long, default_value_t = 10)]
    max_final_candidates: usize,

    /// Log level (trace|debug|info|notice|warning|error|critical).
    #[arg(long, value_enum, default_value_t = LogLevelArg::Notice)]
    log_level: LogLevelArg,
}

#[derive(Clone, Debug, ValueEnum)]
enum IncomingCharsetArg {
    #[value(name = "UTF-8")]
    Utf8,
    #[value(name = "EUC-JP")]
    EucJp,
}

impl IncomingCharsetArg {
    fn into_server(self) -> IncomingCharset {
        match self {
            Self::Utf8 => IncomingCharset::Utf8,
            Self::EucJp => IncomingCharset::EucJp,
        }
    }
}

#[derive(Clone, Debug, ValueEnum)]
enum LogLevelArg {
    Trace,
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
}

impl LogLevelArg {
    fn into_filter(self) -> EnvFilter {
        let directive = match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            // swift-log distinguishes info/notice; tracing collapses to INFO.
            Self::Info | Self::Notice => "info",
            Self::Warning => "warn",
            // swift-log distinguishes error/critical; tracing collapses to ERROR.
            Self::Error | Self::Critical => "error",
        };
        EnvFilter::new(directive)
    }
}

fn main() -> ExitCode {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(args.log_level.into_filter())
        .with_writer(std::io::stderr)
        .init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to start runtime: {}", e);
            return ExitCode::from(1);
        }
    };

    let store = DictionaryStore::new();
    let watcher = Arc::new(UserDictionaryWatcher::new(
        args.user_dictionary.clone(),
        args.system_dictionaries.clone(),
        store.clone(),
    ));

    let server = SkkServer::new(
        skkserv_compound::VERSION,
        "skkserv-compound",
        store,
        CompoundGeneratorConfig::new(args.max_candidates_per_reading, args.max_final_candidates),
    );

    let bind_address = args.bind_address.clone();
    let port = args.port;
    let charset = args.incoming_charset.into_server();
    let watcher_for_async = watcher.clone();

    let result: Result<(), anyhow::Error> = runtime.block_on(async move {
        watcher_for_async.start().await?;
        // Run the server alongside a SIGINT/SIGTERM-style shutdown signal so
        // Ctrl-C unwinds cleanly instead of killing in-flight requests.
        tokio::select! {
            res = server.run(&bind_address, port, charset) => {
                res?;
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal, stopping.");
            }
        }
        watcher_for_async.stop();
        Ok(())
    });

    if let Err(e) = result {
        error!("An error occurred: {}", e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
