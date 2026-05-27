// SPDX-License-Identifier: MIT

use crate::dictionary::Candidate;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PrefixMatch {
    pub length: usize,
    pub reading: String,
}

/// One indexed dictionary tier. The shape is the same for the system and user
/// tiers; what differs is lifetime and contents:
///
/// - The **system** layer is built once at startup and shared by `Arc` for the
///   life of the process; the system dictionaries never change at runtime.
/// - The **user** layer is rebuilt on every user-dictionary change, and its
///   candidate lists are already *merged* with the system tier (user candidates
///   first, then the system's for the same reading, deduped by text). Pre-merging
///   at reload time keeps the query path a plain borrow even when a reading
///   exists in both tiers (the common case for SKK, which writes learned
///   conversions back into the user dictionary).
#[derive(Debug, Default, Clone)]
pub(crate) struct Layer {
    pub entries_by_reading: HashMap<String, Vec<Candidate>>,
    pub readings_by_first_char: HashMap<char, Vec<String>>,
    pub okuri_ari_entries_by_reading: HashMap<String, Vec<Candidate>>,
    pub okuri_ari_readings_by_first_char: HashMap<char, Vec<String>>,
}

impl Layer {
    /// Index a layer from ordered `(reading, candidates)` pairs for the
    /// okuri-nashi and okuri-ari buckets. Input order is preserved in the
    /// per-first-char reading lists (it drives the generator's split-order
    /// tiebreak).
    pub(crate) fn new(
        ordered_entries: Vec<(String, Vec<Candidate>)>,
        ordered_okuri_ari_entries: Vec<(String, Vec<Candidate>)>,
    ) -> Self {
        let (entries_by_reading, readings_by_first_char) = Self::index(ordered_entries);
        let (okuri_ari_entries_by_reading, okuri_ari_readings_by_first_char) =
            Self::index(ordered_okuri_ari_entries);
        Self {
            entries_by_reading,
            readings_by_first_char,
            okuri_ari_entries_by_reading,
            okuri_ari_readings_by_first_char,
        }
    }

    fn index(
        entries: Vec<(String, Vec<Candidate>)>,
    ) -> (HashMap<String, Vec<Candidate>>, HashMap<char, Vec<String>>) {
        let mut by_reading: HashMap<String, Vec<Candidate>> = HashMap::new();
        let mut by_first_char: HashMap<char, Vec<String>> = HashMap::new();
        for (reading, cands) in entries {
            if let Some(first) = reading.chars().next() {
                by_first_char
                    .entry(first)
                    .or_default()
                    .push(reading.clone());
            }
            by_reading.insert(reading, cands);
        }
        (by_reading, by_first_char)
    }
}

#[derive(Debug, Clone)]
pub struct DictionarySnapshot {
    /// Readings the user dictionary contributes, each already merged with the
    /// system tier (user-first, deduped). Rebuilt on every user-dict change.
    user: Layer,
    /// The immutable system tier, built once and shared across reloads.
    system: Arc<Layer>,
}

impl DictionarySnapshot {
    pub(crate) fn from_layers(user: Layer, system: Arc<Layer>) -> Self {
        Self { user, system }
    }

    pub fn candidates(&self, reading: &str) -> &[Candidate] {
        // The user layer's list is pre-merged with the system tier, so a hit
        // there is authoritative; only fall through to system when the user
        // dictionary doesn't mention this reading at all.
        if let Some(cands) = self.user.entries_by_reading.get(reading) {
            return cands.as_slice();
        }
        self.system
            .entries_by_reading
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn okuri_ari_candidates(&self, reading: &str) -> &[Candidate] {
        if let Some(cands) = self.user.okuri_ari_entries_by_reading.get(reading) {
            return cands.as_slice();
        }
        self.system
            .okuri_ari_entries_by_reading
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Every okuri-nashi reading that matches a prefix of `chars[start...]`.
    /// User readings come first (in user order); system readings follow, minus
    /// any reading the user layer already produced — so a reading present in
    /// both tiers appears exactly once, at its user-tier position.
    pub fn prefix_matches(&self, chars: &[char], start: usize) -> Vec<PrefixMatch> {
        if start >= chars.len() {
            return Vec::new();
        }
        let first = chars[start];
        let mut result = Vec::new();
        if let Some(readings) = self.user.readings_by_first_char.get(&first) {
            collect_prefix_matches(readings, chars, start, &mut result);
        }
        let user_seen: HashSet<String> = result.iter().map(|m| m.reading.clone()).collect();
        if let Some(readings) = self.system.readings_by_first_char.get(&first) {
            let mut sys = Vec::new();
            collect_prefix_matches(readings, chars, start, &mut sys);
            for m in sys {
                if !user_seen.contains(&m.reading) {
                    result.push(m);
                }
            }
        }
        result
    }

    /// Every okuri-ari reading whose hiragana stem matches a prefix of
    /// `chars[start...]` AND whose trailing ASCII letter equals `okuri_prefix`.
    /// `length` covers only the hiragana stem (the ASCII letter is in the key,
    /// not in the input). Same cross-tier dedup as `prefix_matches`.
    pub fn okuri_ari_prefix_matches(
        &self,
        chars: &[char],
        start: usize,
        okuri_prefix: char,
    ) -> Vec<PrefixMatch> {
        if start >= chars.len() {
            return Vec::new();
        }
        let first = chars[start];
        let mut result = Vec::new();
        if let Some(readings) = self.user.okuri_ari_readings_by_first_char.get(&first) {
            collect_okuri_ari_prefix_matches(readings, chars, start, okuri_prefix, &mut result);
        }
        let user_seen: HashSet<String> = result.iter().map(|m| m.reading.clone()).collect();
        if let Some(readings) = self.system.okuri_ari_readings_by_first_char.get(&first) {
            let mut sys = Vec::new();
            collect_okuri_ari_prefix_matches(readings, chars, start, okuri_prefix, &mut sys);
            for m in sys {
                if !user_seen.contains(&m.reading) {
                    result.push(m);
                }
            }
        }
        result
    }

    pub fn empty() -> Self {
        Self {
            user: Layer::default(),
            system: Arc::new(Layer::default()),
        }
    }
}

fn collect_prefix_matches(
    readings: &[String],
    chars: &[char],
    start: usize,
    out: &mut Vec<PrefixMatch>,
) {
    let remaining = chars.len() - start;
    for reading in readings {
        let length = reading.chars().count();
        if length > remaining {
            continue;
        }
        if matches_prefix(reading.chars(), chars, start) {
            out.push(PrefixMatch {
                length,
                reading: reading.clone(),
            });
        }
    }
}

fn collect_okuri_ari_prefix_matches(
    readings: &[String],
    chars: &[char],
    start: usize,
    okuri_prefix: char,
    out: &mut Vec<PrefixMatch>,
) {
    let remaining = chars.len() - start;
    for reading in readings {
        if !reading.ends_with(okuri_prefix) {
            continue;
        }
        let total_len = reading.chars().count();
        if total_len == 0 {
            continue;
        }
        let stem_length = total_len - 1;
        if stem_length < 1 || stem_length > remaining {
            continue;
        }
        if matches_prefix(reading.chars().take(stem_length), chars, start) {
            out.push(PrefixMatch {
                length: stem_length,
                reading: reading.clone(),
            });
        }
    }
}

fn matches_prefix<I: IntoIterator<Item = char>>(prefix: I, chars: &[char], start: usize) -> bool {
    for (i, ch) in (start..).zip(prefix) {
        if i >= chars.len() || chars[i] != ch {
            return false;
        }
    }
    true
}
