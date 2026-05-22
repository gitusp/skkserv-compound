// SPDX-License-Identifier: MIT

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DictionarySource {
    User,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Candidate {
    pub text: String,
    pub source: DictionarySource,
}

impl Candidate {
    pub fn new(text: impl Into<String>, source: DictionarySource) -> Self {
        Self {
            text: text.into(),
            source,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEntry {
    pub reading: String,
    pub candidates: Vec<String>,
    pub is_okuri_ari: bool,
}

impl ParsedEntry {
    pub fn new(reading: impl Into<String>, candidates: Vec<String>) -> Self {
        Self {
            reading: reading.into(),
            candidates,
            is_okuri_ari: false,
        }
    }

    pub fn with_okuri(
        reading: impl Into<String>,
        candidates: Vec<String>,
        is_okuri_ari: bool,
    ) -> Self {
        Self {
            reading: reading.into(),
            candidates,
            is_okuri_ari,
        }
    }
}
