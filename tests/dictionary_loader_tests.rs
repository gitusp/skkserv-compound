// SPDX-License-Identifier: MIT

use skkserv_compound::dictionary::{DictionarySource, ParsedEntry};
use skkserv_compound::loader::{DictionaryLoaderError, build_snapshot, read_dictionary_file};
use skkserv_compound::parser::parse;
use skkserv_compound::snapshot::DictionarySnapshot;
use std::fs;
use tempfile::TempDir;

/// File-reading constructor used by the encoding/multi-dictionary tests below.
/// Composes the public read + parse + build primitives (the production server
/// uses the finer-grained layer loaders directly, so the crate no longer ships
/// this convenience wrapper itself).
fn load_snapshot(
    user_dictionary_path: &str,
    system_dictionary_paths: &[String],
) -> Result<DictionarySnapshot, DictionaryLoaderError> {
    let user = parse(&read_dictionary_file(user_dictionary_path)?);
    let mut system = Vec::new();
    for path in system_dictionary_paths {
        system.extend(parse(&read_dictionary_file(path)?));
    }
    Ok(build_snapshot(&user, &system))
}

fn entry(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::new(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
    )
}

fn okuri_entry(reading: &str, candidates: &[&str]) -> ParsedEntry {
    ParsedEntry::with_okuri(
        reading.to_string(),
        candidates.iter().map(|s| s.to_string()).collect(),
        true,
    )
}

#[test]
fn merges_user_and_system() {
    let user = vec![entry("か", &["蚊"])];
    let system = vec![entry("か", &["化"]), entry("せいそう", &["清掃"])];
    let snapshot = build_snapshot(&user, &system);
    let ka: Vec<&str> = snapshot
        .candidates("か")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(ka, vec!["蚊", "化"]);
    let sei: Vec<&str> = snapshot
        .candidates("せいそう")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(sei, vec!["清掃"]);
}

#[test]
fn same_candidate_prefers_user_source() {
    let user = vec![entry("か", &["化"])];
    let system = vec![entry("か", &["化", "蚊"])];
    let snapshot = build_snapshot(&user, &system);
    let cs = snapshot.candidates("か");
    assert_eq!(cs.len(), 2);
    assert_eq!(cs[0].text, "化");
    assert_eq!(cs[0].source, DictionarySource::User);
    assert_eq!(cs[1].text, "蚊");
    assert_eq!(cs[1].source, DictionarySource::System);
}

#[test]
fn user_candidates_precede_system_candidates() {
    let user = vec![entry("あ", &["亜"])];
    let system = vec![entry("あ", &["阿", "唖"])];
    let snapshot = build_snapshot(&user, &system);
    let cs: Vec<&str> = snapshot
        .candidates("あ")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(cs, vec!["亜", "阿", "唖"]);
}

#[test]
fn okuri_ari_entries_go_into_okuri_ari_bucket() {
    let user = vec![okuri_entry("なs", &["無"])];
    let system = vec![okuri_entry("なs", &["済"]), entry("もんだい", &["問題"])];
    let snapshot = build_snapshot(&user, &system);
    let cs = snapshot.okuri_ari_candidates("なs");
    let texts: Vec<&str> = cs.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["無", "済"]);
    assert_eq!(cs[0].source, DictionarySource::User);
    assert_eq!(cs[1].source, DictionarySource::System);
    assert!(snapshot.candidates("なs").is_empty());
    let mond: Vec<&str> = snapshot
        .candidates("もんだい")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(mond, vec!["問題"]);
    // The okuri-ari reading must not leak into the okuri-nashi prefix index:
    // a nashi prefix search from 'な' over "なs" finds nothing.
    assert!(snapshot.prefix_matches(&['な', 's'], 0).is_empty());
}

#[test]
fn okuri_nashi_entries_stay_in_default_bucket() {
    let user = vec![entry("あ", &["亜"])];
    let system = vec![entry("あ", &["阿"])];
    let snapshot = build_snapshot(&user, &system);
    let cs: Vec<&str> = snapshot
        .candidates("あ")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(cs, vec!["亜", "阿"]);
    assert!(snapshot.okuri_ari_candidates("あ").is_empty());
}

#[test]
fn prefers_first_listed_system_dictionary() {
    let dir = TempDir::new().unwrap();
    let user = dir.path().join("user.dict");
    let s1 = dir.path().join("system1.dict");
    let s2 = dir.path().join("system2.dict");
    fs::write(&user, "").unwrap();
    fs::write(&s1, "か /化/\n").unwrap();
    fs::write(&s2, "か /課/\n").unwrap();
    let snapshot = load_snapshot(
        user.to_str().unwrap(),
        &[
            s1.to_str().unwrap().to_string(),
            s2.to_str().unwrap().to_string(),
        ],
    )
    .unwrap();
    let cs = snapshot.candidates("か");
    let texts: Vec<&str> = cs.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["化", "課"]);
    assert!(cs.iter().all(|c| c.source == DictionarySource::System));
}

#[test]
fn duplicate_candidate_goes_to_first_system() {
    let dir = TempDir::new().unwrap();
    let user = dir.path().join("user.dict");
    let s1 = dir.path().join("system1.dict");
    let s2 = dir.path().join("system2.dict");
    fs::write(&user, "").unwrap();
    fs::write(&s1, "か /化/蚊/\n").unwrap();
    fs::write(&s2, "か /課/化/\n").unwrap();
    let snapshot = load_snapshot(
        user.to_str().unwrap(),
        &[
            s1.to_str().unwrap().to_string(),
            s2.to_str().unwrap().to_string(),
        ],
    )
    .unwrap();
    let cs = snapshot.candidates("か");
    let texts: Vec<&str> = cs.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["化", "蚊", "課"]);
    assert!(cs.iter().all(|c| c.source == DictionarySource::System));
}

#[test]
fn merges_okuri_ari_across_system_dictionaries() {
    let dir = TempDir::new().unwrap();
    let user = dir.path().join("user.dict");
    let s1 = dir.path().join("system1.dict");
    let s2 = dir.path().join("system2.dict");
    fs::write(&user, "なs /済/\n").unwrap();
    fs::write(&s1, "なs /無/済/\n").unwrap();
    fs::write(&s2, "なs /為/\n").unwrap();
    let snapshot = load_snapshot(
        user.to_str().unwrap(),
        &[
            s1.to_str().unwrap().to_string(),
            s2.to_str().unwrap().to_string(),
        ],
    )
    .unwrap();
    let cs = snapshot.okuri_ari_candidates("なs");
    let texts: Vec<&str> = cs.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["済", "無", "為"]);
    assert_eq!(cs[0].source, DictionarySource::User);
    assert_eq!(cs[1].source, DictionarySource::System);
    assert_eq!(cs[2].source, DictionarySource::System);
}

#[test]
fn loads_without_system_dictionaries() {
    let dir = TempDir::new().unwrap();
    let user = dir.path().join("user.dict");
    fs::write(&user, "あ /亜/\n").unwrap();
    let snapshot = load_snapshot(user.to_str().unwrap(), &[]).unwrap();
    let cs: Vec<&str> = snapshot
        .candidates("あ")
        .iter()
        .map(|c| c.text.as_str())
        .collect();
    assert_eq!(cs, vec!["亜"]);
}

#[test]
fn prefix_search_returns_matching_readings() {
    let snapshot = build_snapshot(
        &[],
        &[entry("せいそう", &["清掃"]), entry("ぎょうしゃ", &["業者"])],
    );
    let chars: Vec<char> = "せいそうぎょうしゃ".chars().collect();
    let matches = snapshot.prefix_matches(&chars, 0);
    let readings: Vec<&str> = matches.iter().map(|m| m.reading.as_str()).collect();
    assert_eq!(readings, vec!["せいそう"]);
}

#[test]
fn first_character_index_narrows_readings() {
    let snapshot = build_snapshot(
        &[],
        &[
            entry("か", &["化"]),
            entry("かわ", &["川"]),
            entry("き", &["木"]),
        ],
    );
    // A prefix search from 'か' over "かわ" reaches only the 'か'-rooted
    // readings ("か" and "かわ"), never the 'き'-rooted one.
    let mut ka: Vec<String> = snapshot
        .prefix_matches(&['か', 'わ'], 0)
        .into_iter()
        .map(|m| m.reading)
        .collect();
    ka.sort();
    assert_eq!(ka, vec!["か".to_string(), "かわ".to_string()]);
    let ki: Vec<String> = snapshot
        .prefix_matches(&['き'], 0)
        .into_iter()
        .map(|m| m.reading)
        .collect();
    assert_eq!(ki, vec!["き".to_string()]);
}
