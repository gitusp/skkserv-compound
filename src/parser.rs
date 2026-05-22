// SPDX-License-Identifier: MIT

use crate::dictionary::ParsedEntry;
use std::collections::HashSet;

pub fn parse(source: &str) -> Vec<ParsedEntry> {
    let mut result = Vec::new();
    for line in source.lines() {
        if let Some(entry) = parse_line(line) {
            result.push(entry);
        }
    }
    result
}

pub fn parse_line(raw: &str) -> Option<ParsedEntry> {
    let line = raw.strip_suffix('\r').unwrap_or(raw);
    if line.is_empty() {
        return None;
    }
    if line.starts_with(';') {
        return None;
    }

    let space_idx = line.find(' ')?;
    let reading = &line[..space_idx];
    if reading.is_empty() {
        return None;
    }
    // SKK encodes okuri-ari headwords as `<hiragana>+<ASCII lowercase>`
    // (e.g. `おくr`). All-ASCII readings like `mini` are abbrevs and stay in
    // the okuri-nashi bucket.
    let okuri_ari = trailing_okuri(reading).is_some();

    let rest = &line[space_idx + 1..];
    if rest.len() < 2 || !rest.starts_with('/') || !rest.ends_with('/') {
        return None;
    }
    let inner = &rest[1..rest.len() - 1];
    if inner.is_empty() {
        return None;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut texts: Vec<String> = Vec::new();
    // SKK-JISYO.L encodes okuri-ari entries with per-okurigana annotation
    // blocks: `/main/[<okuri-hira>/cand/.../]/[<okuri-hira>/cand/.../]/`.
    // The `[<okuri>` opener and matching `]` closer are structural metadata,
    // not candidate texts — skip them and only emit the candidates inside.
    let mut in_okuri_block = false;
    for raw_piece in inner.split('/') {
        let mut text: &str = raw_piece;
        if let Some(semi) = text.find(';') {
            text = &text[..semi];
        }
        if text.is_empty() {
            continue;
        }
        if okuri_ari {
            // Only treat `[<hiragana>` as a block opener; a candidate that
            // legitimately starts with `[` followed by anything else (e.g.
            // `[英語]亜`) must not be swallowed.
            if let Some(rest) = text.strip_prefix('[')
                && rest
                    .chars()
                    .next()
                    .is_some_and(|c| (0x3041..=0x3096).contains(&(c as u32)))
            {
                in_okuri_block = true;
                continue;
            }
            if in_okuri_block && text == "]" {
                in_okuri_block = false;
                continue;
            }
        }
        if seen.insert(text.to_string()) {
            texts.push(text.to_string());
        }
    }
    if texts.is_empty() {
        return None;
    }
    Some(ParsedEntry::with_okuri(
        reading.to_string(),
        texts,
        okuri_ari,
    ))
}

/// Returns the trailing ASCII lowercase letter if `text` ends with
/// `<hiragana><a-z>`, otherwise None. Hiragana range U+3041..U+3096 matches
/// the SKK okuri-ari stem set.
pub fn trailing_okuri(text: &str) -> Option<char> {
    let mut iter = text.chars().rev();
    let last = iter.next()?;
    if !last.is_ascii_lowercase() {
        return None;
    }
    let before = iter.next()?;
    let u = before as u32;
    if (0x3041..=0x3096).contains(&u) {
        Some(last)
    } else {
        None
    }
}
