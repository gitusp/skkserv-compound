// SPDX-License-Identifier: MIT

use crate::dictionary::{Candidate, DictionarySource, ParsedEntry};
use crate::parser;
use crate::snapshot::DictionarySnapshot;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DictionaryLoaderError {
    #[error("Could not decode dictionary file as UTF-8 or EUC-JP: {0}")]
    EncodingNotRecognized(String),
    #[error("Failed to read dictionary file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

pub fn load_snapshot(
    user_dictionary_path: &str,
    system_dictionary_paths: &[String],
) -> Result<DictionarySnapshot, DictionaryLoaderError> {
    let user_text = read_dictionary_file(user_dictionary_path)?;
    let user_parsed = parser::parse(&user_text);
    let mut system_parsed: Vec<ParsedEntry> = Vec::new();
    for path in system_dictionary_paths {
        let text = read_dictionary_file(path)?;
        system_parsed.extend(parser::parse(&text));
    }
    Ok(build_snapshot(&user_parsed, &system_parsed))
}

pub fn build_snapshot(user: &[ParsedEntry], system: &[ParsedEntry]) -> DictionarySnapshot {
    let mut user_nashi: Vec<&ParsedEntry> = Vec::new();
    let mut user_ari: Vec<&ParsedEntry> = Vec::new();
    for entry in user {
        if entry.is_okuri_ari {
            user_ari.push(entry);
        } else {
            user_nashi.push(entry);
        }
    }
    let mut system_nashi: Vec<&ParsedEntry> = Vec::new();
    let mut system_ari: Vec<&ParsedEntry> = Vec::new();
    for entry in system {
        if entry.is_okuri_ari {
            system_ari.push(entry);
        } else {
            system_nashi.push(entry);
        }
    }
    DictionarySnapshot::new(
        merge_bucket(&user_nashi, &system_nashi),
        merge_bucket(&user_ari, &system_ari),
    )
}

fn merge_bucket(
    user: &[&ParsedEntry],
    system: &[&ParsedEntry],
) -> Vec<(String, Vec<Candidate>)> {
    struct Group {
        candidates: Vec<Candidate>,
        seen: HashSet<String>,
    }
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for (entries, source) in [
        (user, DictionarySource::User),
        (system, DictionarySource::System),
    ] {
        for entry in entries {
            let group = groups.entry(entry.reading.clone()).or_insert_with(|| {
                order.push(entry.reading.clone());
                Group {
                    candidates: Vec::new(),
                    seen: HashSet::new(),
                }
            });
            for text in &entry.candidates {
                if group.seen.insert(text.clone()) {
                    group.candidates.push(Candidate::new(text.clone(), source));
                }
            }
        }
    }

    let mut result: Vec<(String, Vec<Candidate>)> = Vec::with_capacity(order.len());
    for reading in order {
        if let Some(group) = groups.remove(&reading) {
            if !group.candidates.is_empty() {
                result.push((reading, group.candidates));
            }
        }
    }
    result
}

pub fn read_dictionary_file(path: &str) -> Result<String, DictionaryLoaderError> {
    let expanded = expand_tilde(path);
    let bytes = fs::read(&expanded).map_err(|source| DictionaryLoaderError::Io {
        path: path.to_string(),
        source,
    })?;
    decode_with_fallback(&bytes, path)
}

fn decode_with_fallback(bytes: &[u8], path: &str) -> Result<String, DictionaryLoaderError> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(s.to_string());
    }
    let (cow, _enc, had_errors) = encoding_rs::EUC_JP.decode(bytes);
    if !had_errors {
        return Ok(cow.into_owned());
    }
    Err(DictionaryLoaderError::EncodingNotRecognized(path.to_string()))
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            let mut p = PathBuf::from(home);
            p.push(rest);
            return p;
        }
    }
    PathBuf::from(path)
}
