// SPDX-License-Identifier: MIT

use skkserv_compound::dictionary::ParsedEntry;
use skkserv_compound::generator::{CompoundGeneratorConfig, generate};
use skkserv_compound::loader::build_snapshot;
use skkserv_compound::parser::parse;
use skkserv_compound::snapshot::DictionarySnapshot;

fn nashi(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::new(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
    )
}

fn ari(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::with_okuri(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
        true,
    )
}

fn snapshot(user: &[(&str, &[&str])], system: &[(&str, &[&str])]) -> DictionarySnapshot {
    let u: Vec<ParsedEntry> = user.iter().map(|(r, c)| nashi(r, c)).collect();
    let s: Vec<ParsedEntry> = system.iter().map(|(r, c)| nashi(r, c)).collect();
    build_snapshot(&u, &s)
}

fn okuri_snapshot(
    user: &[(&str, &[&str])],
    system: &[(&str, &[&str])],
    okuri_user: &[(&str, &[&str])],
    okuri_system: &[(&str, &[&str])],
) -> DictionarySnapshot {
    let mut u: Vec<ParsedEntry> = user.iter().map(|(r, c)| nashi(r, c)).collect();
    u.extend(okuri_user.iter().map(|(r, c)| ari(r, c)));
    let mut s: Vec<ParsedEntry> = system.iter().map(|(r, c)| nashi(r, c)).collect();
    s.extend(okuri_system.iter().map(|(r, c)| ari(r, c)));
    build_snapshot(&u, &s)
}

#[test]
fn combines_seisou_gyousha() {
    let snap = snapshot(&[], &[("せいそう", &["清掃"]), ("ぎょうしゃ", &["業者"])]);
    let out = generate(
        "せいそうぎょうしゃ",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out.first().map(String::as_str), Some("清掃業者"));
}

#[test]
fn combines_tanjunka() {
    let snap = snapshot(&[], &[("たんじゅん", &["単純"]), ("か", &["化", "蚊"])]);
    let out = generate(
        "たんじゅんか",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert!(out.iter().any(|s| s == "単純化"));
}

#[test]
fn skips_single_word_exact_match() {
    let snap = snapshot(
        &[],
        &[
            ("たんじゅんか", &["単純化"]),
            ("たんじゅん", &["単純"]),
            ("か", &["化"]),
        ],
    );
    let out = generate(
        "たんじゅんか",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert!(out.iter().any(|s| s == "単純化"));
    assert_eq!(out.iter().filter(|s| s.as_str() == "単純化").count(), 1);
}

#[test]
fn prefers_fewer_parts() {
    let snap = snapshot(
        &[],
        &[
            ("せいそう", &["清掃"]),
            ("せい", &["清"]),
            ("そう", &["掃"]),
            ("ぎょうしゃ", &["業者"]),
        ],
    );
    let out = generate(
        "せいそうぎょうしゃ",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out.first().map(String::as_str), Some("清掃業者"));
}

#[test]
fn allows_short_readings() {
    let snap = snapshot(&[], &[("たんじゅん", &["単純"]), ("か", &["化"])]);
    let out = generate(
        "たんじゅんか",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out, vec!["単純化".to_string()]);
}

#[test]
fn respects_final_cap() {
    let snap = snapshot(
        &[],
        &[
            ("たん", &["1", "2", "3", "4", "5"]),
            ("じゅん", &["A", "B", "C", "D", "E"]),
        ],
    );
    let out = generate("たんじゅん", &snap, CompoundGeneratorConfig::new(4), None);
    assert_eq!(out.len(), 4);
}

#[test]
fn dedupes_candidates() {
    let snap = snapshot(
        &[],
        &[
            ("あい", &["AB"]),
            ("う", &["C"]),
            ("あ", &["A"]),
            ("いう", &["BC"]),
        ],
    );
    let out = generate("あいう", &snap, CompoundGeneratorConfig::default(), None);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], "ABC");
}

#[test]
fn does_not_bury_short_suffix_split() {
    // No length/balance heuristic: a derivational split like 構造+化 (whose
    // shortest part is the 1-char suffix か) must NOT be drained behind every
    // candidate of a longer-balanced competing split. Same-k splits round-robin
    // by rank, so 構造化 surfaces in the first round, not after こう+ぞうか is
    // exhausted. `こうぞう` is registered before `こう`, so the 構造+化 split
    // enumerates first and 構造化 leads.
    let snap = snapshot(
        &[],
        &[
            ("こうぞう", &["構造"]),
            ("か", &["化", "課"]),
            ("こう", &["項", "公"]),
            ("ぞうか", &["増加", "造花"]),
        ],
    );
    let out = generate(
        "こうぞうか",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(
        out,
        vec!["構造化", "項増加", "構造課", "項造花", "公増加", "公造花"]
    );
}

#[test]
fn combines_abbrev_katakana() {
    let snap = snapshot(
        &[],
        &[
            ("double", &["ダブル"]),
            ("cheese", &["チーズ"]),
            ("burger", &["バーガー"]),
        ],
    );
    let out = generate(
        "doublecheeseburger",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(
        out.first().map(String::as_str),
        Some("ダブルチーズバーガー")
    );
}

#[test]
fn combines_abbrev_item_card_set() {
    let snap = snapshot(
        &[],
        &[
            ("item", &["アイテム"]),
            ("card", &["カード"]),
            ("set", &["セット"]),
        ],
    );
    let out = generate(
        "itemcardset",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(
        out.first().map(String::as_str),
        Some("アイテムカードセット")
    );
}

#[test]
fn best_first_small_cap_stays_on_top_split() {
    let snap = snapshot(
        &[],
        &[
            ("せいそう", &["清掃", "整層", "正装"]),
            ("せい", &["スイ"]),
            ("そう", &["ソウ"]),
            ("ぎょうしゃ", &["業者"]),
        ],
    );
    let out = generate(
        "せいそうぎょうしゃ",
        &snap,
        CompoundGeneratorConfig::new(2),
        None,
    );
    assert_eq!(out, vec!["清掃業者", "整層業者"]);
    assert!(!out.iter().any(|s| s == "スイソウ業者"));
}

#[test]
fn best_first_retreats_to_lower_split_when_needed() {
    let snap = snapshot(
        &[],
        &[
            ("せいそう", &["清掃"]),
            ("せい", &["清", "整"]),
            ("そう", &["掃", "層"]),
            ("ぎょうしゃ", &["業者"]),
        ],
    );
    let out = generate(
        "せいそうぎょうしゃ",
        &snap,
        CompoundGeneratorConfig::new(3),
        None,
    );
    assert_eq!(out, vec!["清掃業者", "清層業者", "整掃業者"]);
}

#[test]
fn skips_higher_k_once_final_cap_filled() {
    let snap = snapshot(
        &[],
        &[
            ("あい", &["AI"]),
            ("うえ", &["UE"]),
            ("あ", &["A_X"]),
            ("い", &["I_X"]),
        ],
    );
    let out = generate("あいうえ", &snap, CompoundGeneratorConfig::new(1), None);
    assert_eq!(out, vec!["AIUE"]);
    assert!(!out.iter().any(|s| s == "A_XI_XUE"));
}

#[test]
fn combines_four_part_abbrev() {
    let snap = snapshot(
        &[],
        &[
            ("double", &["ダブル"]),
            ("cheese", &["チーズ"]),
            ("burger", &["バーガー"]),
            ("set", &["セット"]),
        ],
    );
    let out = generate(
        "doublecheeseburgerset",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(
        out.first().map(String::as_str),
        Some("ダブルチーズバーガーセット")
    );
}

#[test]
fn skips_k_larger_than_yomi_length() {
    let snap = snapshot(&[], &[("あ", &["X"]), ("い", &["Y"])]);
    let out = generate("あい", &snap, CompoundGeneratorConfig::default(), None);
    assert_eq!(out, vec!["XY".to_string()]);
}

#[test]
fn two_part_beats_three_part_across_k() {
    let snap = snapshot(
        &[],
        &[
            ("あい", &["AB"]),
            ("う", &["C"]),
            ("あ", &["D"]),
            ("いう", &["EF"]),
            ("い", &["G"]),
        ],
    );
    let out = generate("あいう", &snap, CompoundGeneratorConfig::default(), None);
    assert_eq!(out, vec!["ABC", "DEF", "DGC"]);
}

#[test]
fn combines_okuri_ari_compound() {
    let snap = okuri_snapshot(
        &[],
        &[("もんだい", &["問題"])],
        &[],
        &[("なs", &["無", "済"])],
    );
    let out = generate(
        "もんだいな",
        &snap,
        CompoundGeneratorConfig::default(),
        Some("s"),
    );
    assert_eq!(out, vec!["問題無", "問題済"]);
}

#[test]
fn skips_okuri_ari_single_word_exact_match() {
    let snap = okuri_snapshot(&[], &[], &[], &[("はs", &["有"])]);
    let out = generate("は", &snap, CompoundGeneratorConfig::default(), Some("s"));
    assert!(out.is_empty());
}

#[test]
fn okuri_ari_only_at_last_part() {
    let snap = okuri_snapshot(
        &[],
        &[("な", &["菜"]), ("もんだい", &["問題"])],
        &[],
        &[("なs", &["無"])],
    );
    let out = generate(
        "なもんだい",
        &snap,
        CompoundGeneratorConfig::default(),
        Some("s"),
    );
    assert!(out.is_empty());
}

#[test]
fn okuri_prefix_does_not_emit_okuri_nashi_splits() {
    let snap = okuri_snapshot(
        &[],
        &[("もんだい", &["問題"]), ("な", &["菜"])],
        &[],
        &[("なs", &["無"])],
    );
    let out = generate(
        "もんだいな",
        &snap,
        CompoundGeneratorConfig::default(),
        Some("s"),
    );
    assert_eq!(out, vec!["問題無".to_string()]);
    assert!(!out.iter().any(|s| s == "問題菜"));
}

#[test]
fn okuri_prefix_nil_falls_back_to_okuri_nashi() {
    let snap = okuri_snapshot(
        &[],
        &[("もんだい", &["問題"]), ("な", &["菜"])],
        &[],
        &[("なs", &["無"])],
    );
    let out = generate(
        "もんだいな",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out, vec!["問題菜".to_string()]);
}

#[test]
fn round_robins_between_splits_at_same_k() {
    let snap = snapshot(
        &[],
        &[
            ("あい", &["A"]),
            ("うえお", &["B"]),
            ("あいう", &["C"]),
            ("えお", &["D"]),
        ],
    );
    let out = generate(
        "あいうえお",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out, vec!["AB", "CD"]);
}

#[test]
fn mondaina_s_does_not_emit_okuri_ari_bracket_metadata() {
    // Real-world SKK-JISYO.L okuri-ari entries embed per-okurigana annotation
    // blocks (`[し/無/]`). Before the parser fix, the `[し` opener and `]`
    // closer leaked into candidate output as `問題[し` and `問題]`.
    let user = parse("");
    let system = parse(
        "もんだい /問題/\n\
         なs /無/[し/無/]/[さ/無/成/為/]/[せ/無/成/]/[そ/無/成/為/]/\n",
    );
    let snap = build_snapshot(&user, &system);
    let out = generate(
        "もんだいな",
        &snap,
        CompoundGeneratorConfig::default(),
        Some("s"),
    );
    assert!(
        !out.iter().any(|s| s.contains('[') || s.contains(']')),
        "bracket-annotation metadata leaked into output: {:?}",
        out
    );
    assert_eq!(out.first().map(String::as_str), Some("問題無"));
}

#[test]
fn round_robins_zenkengen() {
    let snap = snapshot(
        &[],
        &[
            ("ぜん", &["全"]),
            ("けんげん", &["権限"]),
            ("ぜんけん", &["全権"]),
            ("げん", &["源", "現", "玄", "言", "弦"]),
        ],
    );
    let out = generate(
        "ぜんけんげん",
        &snap,
        CompoundGeneratorConfig::default(),
        None,
    );
    assert_eq!(out.first().map(String::as_str), Some("全権限"));
    assert!(out.iter().any(|s| s == "全権限"));
}
