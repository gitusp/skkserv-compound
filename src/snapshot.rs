// SPDX-License-Identifier: MIT

use crate::dictionary::Candidate;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct PrefixMatch {
    pub length: usize,
    pub reading: String,
}

/// One okuri-nashi or okuri-ari index: candidates keyed by reading, plus the
/// readings grouped by first char for prefix matching.
#[derive(Debug, Default, Clone)]
pub(crate) struct Bucket {
    pub entries_by_reading: HashMap<String, Vec<Candidate>>,
    pub readings_by_first_char: HashMap<char, Vec<String>>,
}

impl Bucket {
    /// Index a bucket from ordered (reading, candidates) pairs. Input order is
    /// preserved in the per-first-char reading lists (drives the generator's
    /// split-order tiebreak).
    fn new(entries: Vec<(String, Vec<Candidate>)>) -> Self {
        let mut entries_by_reading: HashMap<String, Vec<Candidate>> = HashMap::new();
        let mut readings_by_first_char: HashMap<char, Vec<String>> = HashMap::new();
        for (reading, cands) in entries {
            if let Some(first) = reading.chars().next() {
                readings_by_first_char
                    .entry(first)
                    .or_default()
                    .push(reading.clone());
            }
            entries_by_reading.insert(reading, cands);
        }
        Self {
            entries_by_reading,
            readings_by_first_char,
        }
    }
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
    pub nashi: Bucket,
    pub ari: Bucket,
}

impl Layer {
    pub(crate) fn new(
        ordered_entries: Vec<(String, Vec<Candidate>)>,
        ordered_okuri_ari_entries: Vec<(String, Vec<Candidate>)>,
    ) -> Self {
        Self {
            nashi: Bucket::new(ordered_entries),
            ari: Bucket::new(ordered_okuri_ari_entries),
        }
    }
}

#[derive(Debug)]
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

    /// The user layer's list is pre-merged with the system tier, so a hit
    /// there is authoritative; only fall through to system when the user
    /// dictionary doesn't mention this reading at all.
    fn candidates_in<'a>(user: &'a Bucket, system: &'a Bucket, reading: &str) -> &'a [Candidate] {
        if let Some(cands) = user.entries_by_reading.get(reading) {
            return cands.as_slice();
        }
        system
            .entries_by_reading
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn candidates(&self, reading: &str) -> &[Candidate] {
        Self::candidates_in(&self.user.nashi, &self.system.nashi, reading)
    }

    pub fn okuri_ari_candidates(&self, reading: &str) -> &[Candidate] {
        Self::candidates_in(&self.user.ari, &self.system.ari, reading)
    }

    /// Run `collect` over the user readings then the system readings, dropping
    /// any system match whose reading the user layer already produced — so a
    /// reading present in both tiers appears exactly once, at its user-tier
    /// position.
    fn prefix_matches_in(
        user_readings: Option<&Vec<String>>,
        system_readings: Option<&Vec<String>>,
        mut collect: impl FnMut(&[String], &mut Vec<PrefixMatch>),
    ) -> Vec<PrefixMatch> {
        let mut result = Vec::new();
        if let Some(readings) = user_readings {
            collect(readings, &mut result);
        }
        let user_seen: HashSet<String> = result.iter().map(|m| m.reading.clone()).collect();
        if let Some(readings) = system_readings {
            let mut sys = Vec::new();
            collect(readings, &mut sys);
            for m in sys {
                if !user_seen.contains(&m.reading) {
                    result.push(m);
                }
            }
        }
        result
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
        Self::prefix_matches_in(
            self.user.nashi.readings_by_first_char.get(&first),
            self.system.nashi.readings_by_first_char.get(&first),
            |readings, out| collect_prefix_matches(readings, chars, start, out),
        )
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
        Self::prefix_matches_in(
            self.user.ari.readings_by_first_char.get(&first),
            self.system.ari.readings_by_first_char.get(&first),
            |readings, out| {
                collect_okuri_ari_prefix_matches(readings, chars, start, okuri_prefix, out)
            },
        )
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
