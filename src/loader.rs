// SPDX-License-Identifier: MIT

use crate::dictionary::{Candidate, DictionarySource, ParsedEntry};
use crate::parser;
use crate::snapshot::{DictionarySnapshot, Layer};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
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

pub fn build_snapshot(user: &[ParsedEntry], system: &[ParsedEntry]) -> DictionarySnapshot {
    let system_layer = Arc::new(build_system_layer(system));
    let user_layer = build_user_layer(user, &system_layer);
    DictionarySnapshot::from_layers(user_layer, system_layer)
}

/// Read, parse, and index the system dictionaries. Built once at startup and
/// shared across user-dictionary reloads — never re-read while the process runs.
pub(crate) fn load_system_layer(
    system_dictionary_paths: &[String],
) -> Result<Layer, DictionaryLoaderError> {
    let mut system_parsed: Vec<ParsedEntry> = Vec::new();
    for path in system_dictionary_paths {
        let text = read_dictionary_file(path)?;
        system_parsed.extend(parser::parse(&text));
    }
    Ok(build_system_layer(&system_parsed))
}

/// Read, parse, and index the user dictionary, merging each reading's
/// candidates with the (already-built) system tier. Cost scales with the user
/// dictionary, not the system one — this is what runs on every user-dict change.
pub(crate) fn load_user_layer(
    user_dictionary_path: &str,
    system: &Layer,
) -> Result<Layer, DictionaryLoaderError> {
    let user_text = read_dictionary_file(user_dictionary_path)?;
    let user_parsed = parser::parse(&user_text);
    Ok(build_user_layer(&user_parsed, system))
}

pub(crate) fn build_system_layer(system: &[ParsedEntry]) -> Layer {
    let (nashi, ari) = partition_okuri(system);
    Layer::new(
        group_by_reading(&nashi, DictionarySource::System),
        group_by_reading(&ari, DictionarySource::System),
    )
}

pub(crate) fn build_user_layer(user: &[ParsedEntry], system: &Layer) -> Layer {
    let (nashi, ari) = partition_okuri(user);
    let nashi = merge_with_system(
        group_by_reading(&nashi, DictionarySource::User),
        &system.nashi.entries_by_reading,
    );
    let ari = merge_with_system(
        group_by_reading(&ari, DictionarySource::User),
        &system.ari.entries_by_reading,
    );
    Layer::new(nashi, ari)
}

fn partition_okuri(entries: &[ParsedEntry]) -> (Vec<&ParsedEntry>, Vec<&ParsedEntry>) {
    entries.iter().partition(|e| !e.is_okuri_ari)
}

/// Group entries by reading in first-seen order, tagging each candidate with
/// `source` and dropping duplicate texts within a reading. Every parsed entry
/// has at least one candidate, so each group ends up non-empty.
fn group_by_reading(
    entries: &[&ParsedEntry],
    source: DictionarySource,
) -> Vec<(String, Vec<Candidate>)> {
    struct Group {
        candidates: Vec<Candidate>,
        seen: HashSet<String>,
    }
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

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

    let mut result: Vec<(String, Vec<Candidate>)> = Vec::with_capacity(order.len());
    for reading in order {
        if let Some(group) = groups.remove(&reading) {
            result.push((reading, group.candidates));
        }
    }
    result
}

/// Append the system tier's candidates for each user reading after the user's
/// own (deduped by text), reproducing the user-first ordering of the former
/// single-pass merge — but scoped to the readings the user dictionary touches.
fn merge_with_system(
    user_groups: Vec<(String, Vec<Candidate>)>,
    system_by_reading: &HashMap<String, Vec<Candidate>>,
) -> Vec<(String, Vec<Candidate>)> {
    user_groups
        .into_iter()
        .map(|(reading, mut candidates)| {
            if let Some(system_candidates) = system_by_reading.get(&reading) {
                let mut seen: HashSet<String> = candidates.iter().map(|c| c.text.clone()).collect();
                for candidate in system_candidates {
                    if seen.insert(candidate.text.clone()) {
                        candidates.push(candidate.clone());
                    }
                }
            }
            (reading, candidates)
        })
        .collect()
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
    Err(DictionaryLoaderError::EncodingNotRecognized(
        path.to_string(),
    ))
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        let mut p = PathBuf::from(home);
        p.push(rest);
        return p;
    }
    PathBuf::from(path)
}
