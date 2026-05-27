// SPDX-License-Identifier: MIT

use clap::{Parser, ValueEnum};
use log::{error, info};
use skkserv_compound::generator::CompoundGeneratorConfig;
use skkserv_compound::server::{IncomingCharset, SkkServer};
use skkserv_compound::store::DictionaryStore;
use skkserv_compound::watcher::UserDictionaryWatcher;
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "skkserv-compound",
    version = skkserv_compound::VERSION,
    about = "A skkserv that returns compound candidates built from SKK dictionaries."
)]
struct Args {
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

    /// Maximum number of final compound candidates returned.
    #[arg(long, default_value_t = CompoundGeneratorConfig::DEFAULT_MAX_FINAL_CANDIDATES)]
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
    fn into_level(self) -> log::LevelFilter {
        match self {
            Self::Trace => log::LevelFilter::Trace,
            Self::Debug => log::LevelFilter::Debug,
            // swift-log distinguishes info/notice; the log crate collapses to Info.
            Self::Info | Self::Notice => log::LevelFilter::Info,
            Self::Warning => log::LevelFilter::Warn,
            // swift-log distinguishes error/critical; the log crate collapses to Error.
            Self::Error | Self::Critical => log::LevelFilter::Error,
        }
    }
}

fn main() -> ExitCode {
    let args = Args::parse();

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .filter_level(args.log_level.into_level())
        .target(env_logger::Target::Stderr)
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
        CompoundGeneratorConfig::new(args.max_final_candidates),
    );

    let port = args.port;
    let charset = args.incoming_charset.into_server();
    let watcher_for_async = watcher.clone();

    let result: Result<(), Box<dyn std::error::Error>> = runtime.block_on(async move {
        watcher_for_async.start().await?;
        // Run the server alongside a SIGINT/SIGTERM-style shutdown signal so
        // Ctrl-C unwinds cleanly instead of killing in-flight requests.
        tokio::select! {
            res = server.run(port, charset) => {
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
