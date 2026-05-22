// SPDX-License-Identifier: MIT

use crate::generator::{CompoundGeneratorConfig, generate};
use crate::parser::trailing_okuri;
use crate::store::DictionaryStore;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncomingCharset {
    Utf8,
    EucJp,
}

impl IncomingCharset {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::EucJp => "EUC-JP",
        }
    }

    /// Strict decoding: returns None when the byte sequence is invalid for
    /// the chosen charset. Mirrors Swift's `String(data:encoding:)` semantics.
    pub fn decode(&self, bytes: &[u8]) -> Option<String> {
        match self {
            Self::Utf8 => std::str::from_utf8(bytes).ok().map(String::from),
            Self::EucJp => {
                let (cow, _enc, had_errors) = encoding_rs::EUC_JP.decode(bytes);
                if had_errors {
                    None
                } else {
                    Some(cow.into_owned())
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OpcodeResult {
    Close,
    Ignore,
    Reply(String),
}

#[derive(Clone)]
pub struct SkkServer {
    pub version: String,
    pub server_name: String,
    pub store: DictionaryStore,
    pub generator_config: CompoundGeneratorConfig,
}

impl SkkServer {
    pub fn new(
        version: impl Into<String>,
        server_name: impl Into<String>,
        store: DictionaryStore,
        generator_config: CompoundGeneratorConfig,
    ) -> Self {
        Self {
            version: version.into(),
            server_name: server_name.into(),
            store,
            generator_config,
        }
    }

    pub async fn run(
        self,
        host: &str,
        port: u16,
        incoming_charset: IncomingCharset,
    ) -> io::Result<()> {
        let listener = TcpListener::bind(format!("{}:{}", host, port)).await?;
        info!(
            "Server started on {}:{} with incoming charset {}.",
            host,
            port,
            incoming_charset.name()
        );

        loop {
            let (stream, _peer) = listener.accept().await?;
            let server = self.clone();
            let host = host.to_string();
            tokio::spawn(async move {
                if let Err(e) = handle_client(stream, server, host, port, incoming_charset).await {
                    warn!("Hit error: {}", e);
                }
            });
        }
    }

    pub async fn handle_opcode(
        &self,
        opcode: char,
        _operand: &str,
        host: &str,
        port: u16,
    ) -> OpcodeResult {
        match opcode {
            '0' => OpcodeResult::Close,
            '1' => OpcodeResult::Reply(self.candidate_response(_operand).await),
            '2' => OpcodeResult::Reply(format!("{}/{} ", self.server_name, self.version)),
            '3' => {
                // Mirror Swift's `Host.current().localizedName ?? ""`, which on
                // macOS returns the LocalHostName (no trailing `.local`).
                let hostname = local_host_name();
                OpcodeResult::Reply(format!("{}/{}:{} ", hostname, host, port))
            }
            '4' => OpcodeResult::Reply("4\n".to_string()),
            other => {
                warn!("Unsupported opcode: {}", other);
                OpcodeResult::Ignore
            }
        }
    }

    pub async fn candidate_response(&self, raw_yomi: &str) -> String {
        let (body, okuri_prefix) = sanitize_yomi(raw_yomi);
        if body.is_empty() {
            return "4\n".to_string();
        }
        let snapshot = self.store.current();
        let candidates = generate(
            &body,
            &snapshot,
            self.generator_config,
            okuri_prefix.as_deref(),
        );
        if candidates.is_empty() {
            "4\n".to_string()
        } else {
            format!("1/{}/\n", candidates.join("/"))
        }
    }
}

async fn handle_client(
    mut stream: TcpStream,
    server: SkkServer,
    host: String,
    port: u16,
    charset: IncomingCharset,
) -> io::Result<()> {
    let mut pending: Vec<u8> = Vec::with_capacity(256);
    let mut buf = [0u8; 1024];
    'outer: loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..n]);
        let requests = extract_messages(&mut pending, charset);
        for (opcode, operand) in requests {
            match server.handle_opcode(opcode, &operand, &host, port).await {
                OpcodeResult::Close => break 'outer,
                OpcodeResult::Ignore => continue,
                OpcodeResult::Reply(body) => {
                    stream.write_all(body.as_bytes()).await?;
                }
            }
        }
    }
    info!("Connection closed");
    Ok(())
}

/// Slice `buffer` into individual skkserv requests at space (0x20) or LF
/// (0x0A) boundaries. Unterminated trailing bytes are left in `buffer` for
/// the next read.
pub fn extract_messages(buffer: &mut Vec<u8>, charset: IncomingCharset) -> Vec<(char, String)> {
    let mut results: Vec<(char, String)> = Vec::new();
    while let Some(delim_pos) = buffer.iter().position(|&b| b == 0x20 || b == 0x0A) {
        let payload: Vec<u8> = buffer.drain(..delim_pos).collect();
        // Consume the delimiter byte.
        buffer.drain(..1);
        if payload.is_empty() {
            continue;
        }
        let Some(text) = charset.decode(&payload) else {
            warn!("Failed to decode {}-byte request; skipping", payload.len());
            continue;
        };
        let mut iter = text.chars();
        if let Some(opcode) = iter.next() {
            let operand: String = iter.collect();
            results.push((opcode, operand));
        }
    }
    results
}

fn local_host_name() -> String {
    let raw = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_default();
    match raw.strip_suffix(".local") {
        Some(stripped) => stripped.to_string(),
        None => raw,
    }
}

/// Normalize a raw skkserv yomi into a `(body, okuri_prefix)` pair. Trims
/// whitespace, lifts a trailing `<hiragana><a-z>` letter into `okuri_prefix`,
/// and passes all-ASCII (abbrev) inputs through verbatim.
pub fn sanitize_yomi(yomi: &str) -> (String, Option<String>) {
    let trimmed = yomi.trim();
    if trimmed.is_empty() {
        return (String::new(), None);
    }
    if trimmed.is_ascii() {
        return (trimmed.to_string(), None);
    }
    if let Some(okuri) = trailing_okuri(trimmed) {
        // Drop the trailing ASCII letter.
        let mut chars: Vec<char> = trimmed.chars().collect();
        chars.pop();
        let body: String = chars.into_iter().collect();
        return (body, Some(okuri.to_string()));
    }
    (trimmed.to_string(), None)
}
