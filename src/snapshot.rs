// SPDX-License-Identifier: MIT

use crate::dictionary::Candidate;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PrefixMatch {
    pub length: usize,
    pub reading: String,
}

#[derive(Debug, Default, Clone)]
pub struct DictionarySnapshot {
    pub entries_by_reading: HashMap<String, Vec<Candidate>>,
    pub readings_by_first_char: HashMap<char, Vec<String>>,
    pub okuri_ari_entries_by_reading: HashMap<String, Vec<Candidate>>,
    pub okuri_ari_readings_by_first_char: HashMap<char, Vec<String>>,
}

impl DictionarySnapshot {
    pub fn new(
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

    pub fn candidates(&self, reading: &str) -> &[Candidate] {
        self.entries_by_reading
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn readings_starting_with(&self, first: char) -> &[String] {
        self.readings_by_first_char
            .get(&first)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn okuri_ari_candidates(&self, reading: &str) -> &[Candidate] {
        self.okuri_ari_entries_by_reading
            .get(reading)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Every okuri-nashi reading that matches a prefix of `chars[start...]`.
    pub fn prefix_matches(&self, chars: &[char], start: usize) -> Vec<PrefixMatch> {
        if start >= chars.len() {
            return Vec::new();
        }
        let Some(candidates) = self.readings_by_first_char.get(&chars[start]) else {
            return Vec::new();
        };
        let remaining = chars.len() - start;
        let mut result = Vec::new();
        for reading in candidates {
            let length = reading.chars().count();
            if length > remaining {
                continue;
            }
            if Self::matches_prefix(reading.chars(), chars, start) {
                result.push(PrefixMatch {
                    length,
                    reading: reading.clone(),
                });
            }
        }
        result
    }

    /// Every okuri-ari reading whose hiragana stem matches a prefix of
    /// `chars[start...]` AND whose trailing ASCII letter equals `okuri_prefix`.
    /// `length` covers only the hiragana stem (the ASCII letter is in the key,
    /// not in the input).
    pub fn okuri_ari_prefix_matches(
        &self,
        chars: &[char],
        start: usize,
        okuri_prefix: char,
    ) -> Vec<PrefixMatch> {
        if start >= chars.len() {
            return Vec::new();
        }
        let Some(candidates) = self.okuri_ari_readings_by_first_char.get(&chars[start]) else {
            return Vec::new();
        };
        let remaining = chars.len() - start;
        let mut result = Vec::new();
        for reading in candidates {
            let last_char = reading.chars().next_back();
            if last_char != Some(okuri_prefix) {
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
            if Self::matches_prefix(reading.chars().take(stem_length), chars, start) {
                result.push(PrefixMatch {
                    length: stem_length,
                    reading: reading.clone(),
                });
            }
        }
        result
    }

    fn matches_prefix<I: IntoIterator<Item = char>>(
        prefix: I,
        chars: &[char],
        start: usize,
    ) -> bool {
        for (i, ch) in (start..).zip(prefix) {
            if i >= chars.len() || chars[i] != ch {
                return false;
            }
        }
        true
    }

    pub fn empty() -> Self {
        Self::default()
    }
}
