// SPDX-License-Identifier: MIT

use skkserv_compound::parser::parse;

#[test]
fn parses_normal_entries() {
    let entries = parse("たんじゅん /単純/\n");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "たんじゅん");
    assert_eq!(entries[0].candidates, vec!["単純".to_string()]);
}

#[test]
fn ignores_comments() {
    let source = ";; okuri-nasi entries.\n;; これはコメント\nぎょうしゃ /業者/";
    let entries = parse(source);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "ぎょうしゃ");
}

#[test]
fn ignores_empty_lines() {
    let source = "\n\nたん /担/\n\n";
    let entries = parse(source);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "たん");
}

#[test]
fn strips_annotations() {
    let entries = parse("か /化;接尾辞/蚊;昆虫/");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].candidates,
        vec!["化".to_string(), "蚊".to_string()]
    );
}

#[test]
fn ignores_malformed_lines() {
    let source = "broken line without slashes\nだけ /\n/ 候補のみ /\nただしい /正/";
    let entries = parse(source);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "ただしい");
}

#[test]
fn keeps_candidate_order() {
    let entries = parse("か /化/蚊/科/");
    assert_eq!(
        entries[0].candidates,
        vec!["化".to_string(), "蚊".to_string(), "科".to_string()]
    );
}

#[test]
fn keeps_okuri_ari_with_flag() {
    let source = "おくr /送/\nおくり /贈/";
    let entries = parse(source);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].reading, "おくr");
    assert_eq!(entries[0].candidates, vec!["送".to_string()]);
    assert!(entries[0].is_okuri_ari);
    assert_eq!(entries[1].reading, "おくり");
    assert!(!entries[1].is_okuri_ari);
}

#[test]
fn accepts_abbrev_entries() {
    let source = "mini /ミニ/\ngift /ギフト;贈り物/\nitem /アイテム/";
    let entries = parse(source);
    let readings: Vec<&str> = entries.iter().map(|e| e.reading.as_str()).collect();
    assert_eq!(readings, vec!["mini", "gift", "item"]);
    assert_eq!(entries[1].candidates, vec!["ギフト".to_string()]);
    assert!(entries.iter().all(|e| !e.is_okuri_ari));
}

#[test]
fn okuri_ari_requires_hiragana_before_latin() {
    let source = "おくr /送/\nABc /Abc/";
    let entries = parse(source);
    let readings: Vec<&str> = entries.iter().map(|e| e.reading.as_str()).collect();
    assert_eq!(readings, vec!["おくr", "ABc"]);
    assert!(entries[0].is_okuri_ari);
    assert!(!entries[1].is_okuri_ari);
}

#[test]
fn okuri_nashi_has_flag_false() {
    let entries = parse("たんじゅん /単純/\nか /化/蚊/\n");
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| !e.is_okuri_ari));
}

#[test]
fn dedupes_candidates_within_line() {
    let entries = parse("か /化/化/蚊/");
    assert_eq!(
        entries[0].candidates,
        vec!["化".to_string(), "蚊".to_string()]
    );
}

#[test]
fn strips_okuri_ari_bracket_annotations() {
    // SKK-JISYO.L encodes okuri-ari entries with per-okurigana annotation
    // blocks like `[し/無/]`. The `[<okuri>` opener and `]` closer are
    // structural metadata, not candidate texts, and must not surface as
    // standalone candidates.
    let entries = parse("なs /無/[し/無/]/[さ/無/成/為/]/[せ/無/成/]/[そ/無/成/為/]/");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "なs");
    assert!(entries[0].is_okuri_ari);
    assert_eq!(
        entries[0].candidates,
        vec!["無".to_string(), "成".to_string(), "為".to_string()]
    );
}

#[test]
fn okuri_ari_skips_structural_block_tokens() {
    // A standard okuri-ari entry with per-okurigana annotation blocks: the
    // real candidates are emitted, but the `[<okuri>` openers and `]` closers
    // are structural and must NOT appear as candidates.
    let entries = parse("おくr /送/[り/送/]/[る/送/]/");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].reading, "おくr");
    assert!(entries[0].is_okuri_ari);
    assert_eq!(entries[0].candidates, vec!["送".to_string()]);
    assert!(!entries[0].candidates.iter().any(|c| c == "[り"));
    assert!(!entries[0].candidates.iter().any(|c| c == "]"));
}

#[test]
fn okuri_ari_keeps_candidate_with_bracket_hiragana_then_non_hiragana() {
    // `[あ]対` starts with `[` + hiragana (`あ`) but is followed by a
    // non-hiragana char (`]`), so the remainder after `[` is not pure
    // hiragana. Under the old `.next().is_some_and(is_hiragana)` rule this
    // candidate was wrongly swallowed as a block opener; it must now be kept.
    let entries = parse("あたいs /価/[あ]対/[す/価/]/");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_okuri_ari);
    assert!(
        entries[0].candidates.iter().any(|c| c == "[あ]対"),
        "expected `[あ]対` to be kept as a candidate, got {:?}",
        entries[0].candidates
    );
    assert!(entries[0].candidates.iter().any(|c| c == "価"));
    assert!(!entries[0].candidates.iter().any(|c| c == "]"));
}

#[test]
fn okuri_ari_keeps_candidate_with_bracket_non_hiragana() {
    // `[英語]亜` starts with `[` + a non-hiragana char, so it was never a
    // block opener under either rule — confirm it is still kept.
    let entries = parse("あs /亜/[英語]亜/[す/亜/]/");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].is_okuri_ari);
    assert!(
        entries[0].candidates.iter().any(|c| c == "[英語]亜"),
        "expected `[英語]亜` to be kept as a candidate, got {:?}",
        entries[0].candidates
    );
}
