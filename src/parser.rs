// SPDX-License-Identifier: MIT

use crate::dictionary::ParsedEntry;
use std::collections::HashSet;

/// True if `c` is in the SKK okuri-ari stem hiragana range (U+3041..=U+3096).
fn is_hiragana(c: char) -> bool {
    ('\u{3041}'..='\u{3096}').contains(&c)
}

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
            // A token is an okuri-block opener only when it is `[` followed by
            // one-or-more okurigana hiragana and nothing else (e.g. `[り`,
            // `[った`). Any candidate containing a non-hiragana char after the
            // `[` (e.g. `[英語]亜`, `[あ]対`) is kept as a candidate.
            if let Some(rest) = text.strip_prefix('[')
                && !rest.is_empty()
                && rest.chars().all(is_hiragana)
            {
                in_okuri_block = true;
                continue;
            }
            // Irreducible assumption: a bare `]` is treated as the block
            // terminator. The SKK format has no escaping for `[`/`]`, so a
            // candidate whose text is literally `]` cannot be distinguished
            // from a structural closer — this is not fixable without a
            // format-level change. Standard SKK dictionaries never use a bare
            // `]` (or a pure `[`+hiragana token) as candidate text.
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
    if is_hiragana(before) {
        Some(last)
    } else {
        None
    }
}
