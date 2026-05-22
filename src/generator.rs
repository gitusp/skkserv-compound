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
    min_part_len: usize,
    enum_order: usize,
}

#[derive(Debug, Eq, PartialEq)]
struct PqEntry {
    split_idx: usize,
    rank: usize,
    indices: Vec<usize>,
    text: String,
    // Cached split min_part_len for ordering — set when pushed.
    min_part_len: usize,
}

// BinaryHeap is a max-heap; we want pop order to follow Swift's MinHeap
// comparator: (min_part_len DESC, rank ASC, split_idx ASC). So an entry is
// "greater" when it should be popped first.
impl Ord for PqEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.min_part_len.cmp(&other.min_part_len) {
            Ordering::Equal => {}
            ord => return ord, // larger min_part_len => greater => popped first
        }
        match self.rank.cmp(&other.rank) {
            Ordering::Equal => {}
            ord => return ord.reverse(), // smaller rank => greater => popped first
        }
        self.split_idx.cmp(&other.split_idx).reverse()
    }
}

impl PartialOrd for PqEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn expand(
    mut splits: Vec<SplitInfo>,
    final_cap: usize,
    seen: &mut HashSet<String>,
    result: &mut Vec<String>,
) {
    // All splits in this batch share the same num_parts; pre-sort by the
    // within-k priority key so that smaller split_idx already encodes
    // (min_part_len DESC, enum_order ASC). The heap comparator then only needs
    // split_idx as the final tiebreaker.
    splits.sort_by(compare_split_key);

    let mut heap: BinaryHeap<PqEntry> = BinaryHeap::new();
    for (idx, info) in splits.iter().enumerate() {
        let indices = vec![0usize; info.num_parts];
        let text = build_text(info, &indices);
        heap.push(PqEntry {
            split_idx: idx,
            rank: 0,
            indices,
            text,
            min_part_len: info.min_part_len,
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
                min_part_len: info.min_part_len,
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
    let mut enum_order = 0usize;
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
        &mut enum_order,
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
    enum_order: &mut usize,
) {
    let n = chars.len();
    if depth == k {
        if start == n
            && let Some(info) =
                make_split(parts, snapshot, cap, *enum_order, okuri_prefix.is_some())
        {
            splits.push(info);
            *enum_order += 1;
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
            enum_order,
        );
        parts.pop();
    }
}

fn make_split(
    parts: &[String],
    snapshot: &DictionarySnapshot,
    cap: usize,
    enum_order: usize,
    last_is_okuri_ari: bool,
) -> Option<SplitInfo> {
    if parts.is_empty() {
        return None;
    }
    // For okuri-ari mode the last part's dictionary key embeds the trailing
    // ASCII letter (e.g. `なs`), but the input only contributes the hiragana
    // stem. Score the split by input-consumed length so it competes fairly
    // against okuri-nashi splits of the same body.
    let mut part_lens: Vec<usize> = parts.iter().map(|p| p.chars().count()).collect();
    if last_is_okuri_ari {
        let last = part_lens.len() - 1;
        // An okuri-ari reading must be `<hiragana stem><ASCII letter>`, so
        // the raw length is at least 2 and the stem length is at least 1.
        // Refuse the split if the upstream invariants are ever violated
        // (e.g. by a future caller constructing snapshots directly), rather
        // than silently producing a length-zero stem.
        let raw = part_lens[last];
        if raw < 2 {
            return None;
        }
        part_lens[last] = raw - 1;
    }
    let min_part_len = *part_lens.iter().min()?;
    let mut part_candidates: Vec<Vec<Candidate>> = Vec::with_capacity(parts.len());
    for (i, reading) in parts.iter().enumerate() {
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
        min_part_len,
        enum_order,
    })
}

fn build_text(info: &SplitInfo, indices: &[usize]) -> String {
    let mut s = String::new();
    for (part_idx, &cand_idx) in indices.iter().enumerate() {
        s.push_str(&info.part_candidates[part_idx][cand_idx].text);
    }
    s
}

fn compare_split_key(a: &SplitInfo, b: &SplitInfo) -> Ordering {
    // Within a single expand() call all splits share num_parts; cross-k
    // ordering is enforced by the outer k loop in generate(). The only
    // hierarchical within-k signal is min_part_len (longer shortest part
    // wins); enum_order is the deterministic fallback.
    if a.min_part_len != b.min_part_len {
        return b.min_part_len.cmp(&a.min_part_len);
    }
    a.enum_order.cmp(&b.enum_order)
}
