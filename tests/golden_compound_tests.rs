// SPDX-License-Identifier: MIT
//
// GOLDEN behavioral snapshot of the generator (index-sum / diagonal rank, no
// per-reading cap). A diff tool that makes every change in observable output
// VISIBLE and reviewable, scenario by scenario. Scenarios A/B/C/G/H exercise
// the diagonal expansion and cap-free sweeps; D/E/F are invariants that pin the
// k-axis (fewer parts first) and round-robin-across-splits behavior.
//
// To (re)capture: run `cargo test --test golden_compound_tests -- --nocapture
// dump_golden` and paste the printed literals into the scenarios below.

use skkserv_compound::dictionary::ParsedEntry;
use skkserv_compound::generator::{CompoundGeneratorConfig, generate};
use skkserv_compound::loader::build_snapshot;
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

fn snap_nashi(system: &[(&str, &[&str])]) -> DictionarySnapshot {
    let s: Vec<ParsedEntry> = system.iter().map(|(r, c)| nashi(r, c)).collect();
    build_snapshot(&[], &s)
}

fn snap_okuri(system: &[(&str, &[&str])], okuri_system: &[(&str, &[&str])]) -> DictionarySnapshot {
    let mut s: Vec<ParsedEntry> = system.iter().map(|(r, c)| nashi(r, c)).collect();
    s.extend(okuri_system.iter().map(|(r, c)| ari(r, c)));
    build_snapshot(&[], &s)
}

/// Each scenario returns (name, output) for the generator under test.
fn run_all() -> Vec<(&'static str, Vec<String>)> {
    vec![
        // A. Single 2-part split, many candidates only on the RIGHT part. Left
        //    has one candidate so it never advances: the whole right list sweeps.
        (
            "A_one_sided_sweep_right",
            generate(
                "あい",
                &snap_nashi(&[
                    ("あ", &["X"]),
                    ("い", &["A", "B", "C", "D", "E", "F", "G", "H"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // B. Single 2-part split, 4 candidates on BOTH parts: the clearest view
        //    of the diagonal (sum-of-indices) rank ordering.
        (
            "B_both_sides_4x4",
            generate(
                "あい",
                &snap_nashi(&[
                    ("あ", &["a0", "a1", "a2", "a3"]),
                    ("い", &["b0", "b1", "b2", "b3"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // C. Single 3-part split, 3 candidates each: the rank-1 band bumps each
        //    of the three parts once (no part is starved).
        (
            "C_three_part_3x3x3",
            generate(
                "あいう",
                &snap_nashi(&[
                    ("あ", &["a0", "a1", "a2"]),
                    ("い", &["b0", "b1", "b2"]),
                    ("う", &["c0", "c1", "c2"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // D. Multiple splits at the same k, each part single-candidate.
        //    Round-robin by rank-0 only.
        (
            "D_multi_split_roundrobin",
            generate(
                "あいうえお",
                &snap_nashi(&[
                    ("あい", &["A"]),
                    ("うえお", &["B"]),
                    ("あいう", &["C"]),
                    ("えお", &["D"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // E. Fewer parts first across k (2-part split must precede 3-part split).
        (
            "E_fewer_parts_first",
            generate(
                "あいう",
                &snap_nashi(&[
                    ("あい", &["AB"]),
                    ("う", &["C"]),
                    ("あ", &["D"]),
                    ("いう", &["EF"]),
                    ("い", &["G"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // F. Two splits at same k; one split's last part has many homophones.
        //    Round-robin across splits interleaved with the within-split sweep.
        (
            "F_roundrobin_with_homophones",
            generate(
                "ぜんけんげん",
                &snap_nashi(&[
                    ("ぜん", &["全"]),
                    ("けんげん", &["権限"]),
                    ("ぜんけん", &["全権"]),
                    ("げん", &["源", "現", "玄", "言", "弦"]),
                ]),
                CompoundGeneratorConfig::default(),
                None,
            ),
        ),
        // G. okuri-ari: single split, last part has many candidates; all surface
        //    (no cap), left fixed.
        (
            "G_okuri_ari_sweep",
            generate(
                "もんだいな",
                &snap_okuri(
                    &[("もんだい", &["問題"])],
                    &[("なs", &["無", "済", "為", "成", "鳴", "生"])],
                ),
                CompoundGeneratorConfig::default(),
                Some("s"),
            ),
        ),
        // H. Explicit small final cap with both-sides homophones: shows which
        //    diagonal rank bands the cap admits.
        (
            "H_final_cap_6_both_sides",
            generate(
                "あい",
                &snap_nashi(&[("あ", &["a0", "a1", "a2"]), ("い", &["b0", "b1", "b2"])]),
                CompoundGeneratorConfig::new(6),
                None,
            ),
        ),
    ]
}

/// Captured behavior of the generator (index-sum / diagonal rank, no cap).
/// Regenerate with the `dump_golden` test below.
#[rustfmt::skip]
const GOLDEN: &[(&str, &[&str])] = &[
    // No cap: the whole right candidate list sweeps (left has one candidate).
    ("A_one_sided_sweep_right", &["XA", "XB", "XC", "XD", "XE", "XF", "XG", "XH"]),
    // Diagonal: rank = sum of indices; (0,1) and (1,0) share rank 1, etc.
    ("B_both_sides_4x4", &["a0b0", "a0b1", "a1b0", "a0b2", "a1b1", "a2b0", "a0b3", "a1b2", "a2b1", "a3b0"]),
    // Diagonal in 3 parts: rank-1 band bumps each of the three parts once.
    ("C_three_part_3x3x3", &["a0b0c0", "a0b0c1", "a0b1c0", "a1b0c0", "a0b0c2", "a0b1c1", "a0b2c0", "a1b0c1", "a1b1c0", "a2b0c0"]),
    // INVARIANT: round-robin across same-k splits, rank-0 only.
    ("D_multi_split_roundrobin", &["AB", "CD"]),
    // INVARIANT: fewer parts first across k.
    ("E_fewer_parts_first", &["ABC", "DEF", "DGC"]),
    // INVARIANT: left parts single-candidate, so the sweep is unambiguous.
    ("F_roundrobin_with_homophones", &["全権限", "全権源", "全権現", "全権玄", "全権言", "全権弦"]),
    // No cap: all six okuri-ari last-part candidates surface (left fixed).
    ("G_okuri_ari_sweep", &["問題無", "問題済", "問題為", "問題成", "問題鳴", "問題生"]),
    // Diagonal under an explicit final cap of 6: the first two rank bands.
    ("H_final_cap_6_both_sides", &["a0b0", "a0b1", "a1b0", "a0b2", "a1b1", "a2b0"]),
];

#[test]
fn golden_matches_current() {
    let actual = run_all();
    assert_eq!(
        actual.len(),
        GOLDEN.len(),
        "scenario count drifted from the golden table"
    );
    for ((name, out), (g_name, g_out)) in actual.iter().zip(GOLDEN.iter()) {
        assert_eq!(name, g_name, "scenario order/name drifted");
        let got: Vec<&str> = out.iter().map(String::as_str).collect();
        assert_eq!(&got, g_out, "output drift in scenario {name}");
    }
}

/// Print every scenario as a paste-ready Rust literal (for re-capturing GOLDEN).
#[test]
fn dump_golden() {
    for (name, out) in run_all() {
        let items: Vec<String> = out.iter().map(|s| format!("{s:?}")).collect();
        println!("    (\"{name}\", &[{}]),", items.join(", "));
    }
}
