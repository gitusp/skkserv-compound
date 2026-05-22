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
    assert_eq!(entries[0].candidates, vec!["化".to_string(), "蚊".to_string()]);
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
    assert_eq!(entries[0].candidates, vec!["化".to_string(), "蚊".to_string()]);
}
