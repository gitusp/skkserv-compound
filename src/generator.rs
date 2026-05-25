// SPDX-License-Identifier: MIT

use crate::dictionary::Candidate;
use crate::snapshot::DictionarySnapshot;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

#[derive(Debug, Clone, Copy)]
pub struct CompoundGeneratorConfig {
    pub max_candidates_per_reading: usize,
    pub max_final_candidates: usize,
}

impl Default for CompoundGeneratorConfig {
    fn default() -> Self {
        Self {
            max_candidates_per_reading: 5,
            max_final_candidates: 10,
        }
    }
}

impl CompoundGeneratorConfig {
    pub fn new(max_candidates_per_reading: usize, max_final_candidates: usize) -> Self {
        Self {
            max_candidates_per_reading,
            max_final_candidates,
        }
    }
}

pub fn generate(
    yomi: &str,
    snapshot: &DictionarySnapshot,
    config: CompoundGeneratorConfig,
    okuri_prefix: Option<&str>,
) -> Vec<String> {
    let chars: Vec<char> = yomi.chars().collect();
    let n = chars.len();
    // A compound by definition requires at least two reading parts. The same
    // floor applies in okuri-ari mode: at least one okuri-nashi prefix part
    // plus one okuri-ari stem part.
    if n < 2 {
        return Vec::new();
    }

    // okuri-ari mode is opted into when the SKK request carried a trailing
    // okurigana romaji marker (e.g. `もんだいなs` → body `もんだいな` + `s`).
    // The marker drives the dictionary bucket used for the last split part.
    let okuri_char: Option<char> = okuri_prefix.and_then(|s| s.chars().next());

    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::with_capacity(config.max_final_candidates);

    // The outer best-first axis is k, the number of reading parts. k = 1 is
    // intentionally skipped: single-word exact matches are returned by the SKK
    // client's own dictionary lookup, not by this compound server. We expand k
    // upward only while the dedupe'd output is still short of the final cap, so
    // larger k stages are never enumerated when smaller k already fills the cap.
    let mut k = 2usize;
    while k <= n && result.len() < config.max_final_candidates {
        let splits = enumerate_splits(
            k,
            &chars,
            snapshot,
            config.max_candidates_per_reading,
            okuri_char,
        );
        if !splits.is_empty() {
            expand(splits, config.max_final_candidates, &mut seen, &mut result);
        }
        k += 1;
    }

    result
}

#[derive(Debug, Clone)]
struct SplitInfo {
    part_candidates: Vec<Vec<Candidate>>,
    num_parts: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct PqEntry {
    split_idx: usize,
    rank: usize,
    indices: Vec<usize>,
    text: String,
}

// BinaryHeap is a max-heap, but the pop order we want is (rank ASC, split_idx
// ASC): a round-robin across every split of the current k — all splits' rank-0
// combination first, then all rank-1, and so on — with split_idx (which equals
// enumeration order, i.e. dictionary registration order) as the deterministic
// tiebreak within a rank. There is deliberately no length/balance heuristic:
// per the project's "no semantic judgement" stance, ordering beyond
// fewer-parts-first is left to dictionary order and user-dictionary learning.
// So an entry is "greater" (popped first) when its rank is smaller, or on equal
// rank when its split_idx is smaller.
impl Ord for PqEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.rank.cmp(&other.rank) {
            Ordering::Equal => self.split_idx.cmp(&other.split_idx).reverse(),
            ord => ord.reverse(), // smaller rank => greater => popped first
        }
    }
}

impl PartialOrd for PqEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn expand(
    splits: Vec<SplitInfo>,
    final_cap: usize,
    seen: &mut HashSet<String>,
    result: &mut Vec<String>,
) {
    // `splits` arrives in enumeration order (dictionary registration order); the
    // index into it is used directly as split_idx, the heap's only tiebreak.
    let mut heap: BinaryHeap<PqEntry> = BinaryHeap::new();
    for (idx, info) in splits.iter().enumerate() {
        let indices = vec![0usize; info.num_parts];
        let text = build_text(info, &indices);
        heap.push(PqEntry {
            split_idx: idx,
            rank: 0,
            indices,
            text,
        });
    }

    while result.len() < final_cap {
        let Some(entry) = heap.pop() else { break };
        if seen.insert(entry.text.clone()) {
            result.push(entry.text.clone());
            if result.len() >= final_cap {
                return;
            }
        }
        // Push the next combination within this split, in lex order with the
        // rightmost index varying fastest. rank increments by one so this
        // entry queues behind every other split's same-rank candidate.
        let info = &splits[entry.split_idx];
        let mut next = entry.indices.clone();
        let mut i: isize = info.num_parts as isize - 1;
        let mut wrapped_all = true;
        while i >= 0 {
            next[i as usize] += 1;
            if next[i as usize] < info.part_candidates[i as usize].len() {
                wrapped_all = false;
                break;
            }
            next[i as usize] = 0;
            i -= 1;
        }
        if !wrapped_all {
            let text = build_text(info, &next);
            heap.push(PqEntry {
                split_idx: entry.split_idx,
                rank: entry.rank + 1,
                indices: next,
                text,
            });
        }
    }
}

fn enumerate_splits(
    k: usize,
    chars: &[char],
    snapshot: &DictionarySnapshot,
    cap: usize,
    okuri_prefix: Option<char>,
) -> Vec<SplitInfo> {
    let mut splits: Vec<SplitInfo> = Vec::new();
    let mut parts: Vec<String> = Vec::with_capacity(k);
    enumerate_recursive(
        k,
        0,
        0,
        chars,
        snapshot,
        cap,
        okuri_prefix,
        &mut parts,
        &mut splits,
    );
    splits
}

#[allow(clippy::too_many_arguments)]
fn enumerate_recursive(
    k: usize,
    depth: usize,
    start: usize,
    chars: &[char],
    snapshot: &DictionarySnapshot,
    cap: usize,
    okuri_prefix: Option<char>,
    parts: &mut Vec<String>,
    splits: &mut Vec<SplitInfo>,
) {
    let n = chars.len();
    if depth == k {
        if start == n
            && let Some(info) = make_split(parts, snapshot, cap, okuri_prefix.is_some())
        {
            splits.push(info);
        }
        return;
    }
    // Each remaining part needs at least one character of yomi left.
    let remaining_parts = k - depth;
    if (n - start) < remaining_parts {
        return;
    }

    // Only the final part may consume the okuri-ari bucket; intermediate parts
    // must come from the okuri-nashi bucket per spec.
    let is_last = depth == k - 1;
    let matches = if is_last {
        match okuri_prefix {
            Some(op) => snapshot.okuri_ari_prefix_matches(chars, start, op),
            None => snapshot.prefix_matches(chars, start),
        }
    } else {
        snapshot.prefix_matches(chars, start)
    };

    for m in matches {
        let next_start = start + m.length;
        if is_last {
            // The final part must consume the entire body. (For okuri-nashi
            // mode the depth == k terminal check also enforces this, but
            // making it explicit keeps the okuri-ari branch tidy.)
            if next_start != n {
                continue;
            }
        } else if (n - next_start) < (remaining_parts - 1) {
            continue;
        }
        parts.push(m.reading);
        enumerate_recursive(
            k,
            depth + 1,
            next_start,
            chars,
            snapshot,
            cap,
            okuri_prefix,
            parts,
            splits,
        );
        parts.pop();
    }
}

fn make_split(
    parts: &[String],
    snapshot: &DictionarySnapshot,
    cap: usize,
    last_is_okuri_ari: bool,
) -> Option<SplitInfo> {
    if parts.is_empty() {
        return None;
    }
    let mut part_candidates: Vec<Vec<Candidate>> = Vec::with_capacity(parts.len());
    for (i, reading) in parts.iter().enumerate() {
        // For okuri-ari mode the last part is keyed in the okuri-ari bucket
        // (e.g. `なs`); every other part comes from the okuri-nashi bucket.
        let is_last_okuri_ari = last_is_okuri_ari && (i == parts.len() - 1);
        let all = if is_last_okuri_ari {
            snapshot.okuri_ari_candidates(reading)
        } else {
            snapshot.candidates(reading)
        };
        let taken: Vec<Candidate> = if all.len() <= cap {
            all.to_vec()
        } else {
            all[..cap].to_vec()
        };
        part_candidates.push(taken);
    }
    if part_candidates.iter().any(|c| c.is_empty()) {
        return None;
    }
    Some(SplitInfo {
        part_candidates,
        num_parts: parts.len(),
    })
}

fn build_text(info: &SplitInfo, indices: &[usize]) -> String {
    let mut s = String::new();
    for (part_idx, &cand_idx) in indices.iter().enumerate() {
        s.push_str(&info.part_candidates[part_idx][cand_idx].text);
    }
    s
}
