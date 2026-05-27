// SPDX-License-Identifier: MIT

//! Compound-candidate generation.
//!
//! Given a `yomi` reading, this module enumerates *compound* candidates: a
//! compound is built by partitioning the **entire** yomi into `k >= 2`
//! contiguous parts, where each part is a dictionary reading key. Picking one
//! candidate string per part and concatenating them (in part order) yields one
//! output text. (`k = 1`, i.e. a single whole-word match, is left to the SKK
//! client's own dictionary lookup and is never produced here.)
//!
//! # Ordering (best-first)
//!
//! Results are emitted under a strict total order:
//!
//! 1. **Primary — `k` ascending.** Every `k = 2` result precedes every
//!    `k = 3` result, and so on. As an optimization we only advance to `k + 1`
//!    while fewer than `max_final_candidates` distinct texts have been
//!    collected; once a smaller `k` fills the cap, larger `k` is never
//!    enumerated.
//!
//! 2. **Within a fixed `k`,** across all valid splits and all their
//!    combinations:
//!    - `rank` = the **sum of the chosen candidate indices**, ascending. A
//!      candidate index is its position in that part's candidate slice
//!      (`0` = top priority). This is a *balanced / diagonal* enumeration: a
//!      second choice on any single part (rank 1) always outranks a deep choice
//!      concentrated on one part (rank >= 2). There is deliberately no
//!      length/balance/semantic heuristic — ordering beyond fewer-parts-first
//!      is left to dictionary order and user-dictionary learning.
//!    - **Tiebreak 1** (equal rank): split enumeration order, ascending. Splits
//!      enumerate by extending part boundaries left-to-right in the order
//!      `prefix_matches` / `okuri_ari_prefix_matches` return readings (i.e.
//!      dictionary registration order).
//!    - **Tiebreak 2** (equal rank *and* same split): the index tuple compared
//!      lexicographically, ascending. (Disambiguates e.g. (0,2) vs (1,1) vs
//!      (2,0), which all share rank 2.)
//!
//! 3. **Dedup by output text** across the whole run: a text already emitted by
//!    any earlier split or `k` is skipped. Collection stops at
//!    `max_final_candidates` distinct texts.
//!
//! # Algorithm
//!
//! For a fixed `k` we run a lazy best-first walk over combinations using a
//! binary min-heap (`BinaryHeap<Reverse<State>>`). Each split is seeded with its
//! all-zero index tuple. Each pop yields the globally smallest unexpanded state
//! under the `(rank, split order, index tuple)` key; we then push its
//! successors — the tuple with exactly one index position bumped by one. A tuple
//! is reachable by bumping different positions, so to generate each exactly once
//! every state only bumps positions at or after a recorded `frontier` index
//! (bumping position `p` sets the successor's frontier to `p`). This is the
//! standard lazy "k-smallest sums" enumeration: it never materializes the full
//! cartesian product and stops the instant the cap is met.

use crate::dictionary::Candidate;
use crate::snapshot::DictionarySnapshot;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};

#[derive(Debug, Clone, Copy)]
pub struct CompoundGeneratorConfig {
    pub max_final_candidates: usize,
}

impl Default for CompoundGeneratorConfig {
    fn default() -> Self {
        Self {
            max_final_candidates: Self::DEFAULT_MAX_FINAL_CANDIDATES,
        }
    }
}

impl CompoundGeneratorConfig {
    pub const DEFAULT_MAX_FINAL_CANDIDATES: usize = 10;

    pub fn new(max_final_candidates: usize) -> Self {
        Self {
            max_final_candidates,
        }
    }
}

/// A valid split of the whole yomi into `k` parts, in part order. Each part is
/// a borrowed slice of the snapshot's candidates for that part's reading key
/// (registration order, index 0 = top priority); nothing is cloned.
struct Split<'a> {
    parts: Vec<&'a [Candidate]>,
}

/// A point in the per-`k` lazy enumeration: one candidate index per part.
struct State {
    /// Sum of `indices` — the rank.
    rank: usize,
    /// Which split this combination belongs to (enumeration order).
    split_order: usize,
    /// Chosen candidate index per part.
    indices: Vec<usize>,
    /// Successors only bump positions `>= frontier`, preventing duplicates.
    frontier: usize,
}

impl State {
    fn ordering_key(&self) -> (usize, usize, &[usize]) {
        (self.rank, self.split_order, self.indices.as_slice())
    }
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.ordering_key() == other.ordering_key()
    }
}

impl Eq for State {}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        // (rank, split order, index tuple) all ascending => "smaller" is better.
        // The heap is a max-heap, so callers wrap states in `Reverse`.
        let (ar, ao, ai) = self.ordering_key();
        let (br, bo, bi) = other.ordering_key();
        ar.cmp(&br).then(ao.cmp(&bo)).then(ai.cmp(bi))
    }
}

/// Generate compound candidates for `yomi`, best-first, capped at
/// `config.max_final_candidates` distinct output texts.
///
/// `okuri_char`: when `Some`, every split's last part must be an okuri-ari
/// key whose hiragana stem ends exactly at the end of the yomi, and whose
/// trailing ASCII letter equals `okuri_char`; all other parts are
/// okuri-nashi. When `None`, every part is okuri-nashi.
pub fn generate(
    yomi: &str,
    snapshot: &DictionarySnapshot,
    config: CompoundGeneratorConfig,
    okuri_char: Option<char>,
) -> Vec<String> {
    let chars: Vec<char> = yomi.chars().collect();
    let n = chars.len();
    // A compound by definition requires at least two reading parts. The same
    // floor applies in okuri-ari mode: at least one okuri-nashi prefix part
    // plus one okuri-ari stem part.
    if n < 2 || config.max_final_candidates == 0 {
        return Vec::new();
    }

    // okuri-ari mode is opted into when the SKK request carried a trailing
    // okurigana romaji marker (e.g. `もんだいなs` → body `もんだいな` + `s`).
    // The marker drives the dictionary bucket used for the last split part.
    let mut results: Vec<String> = Vec::with_capacity(config.max_final_candidates);
    let mut seen: HashSet<String> = HashSet::new();

    // Primary axis: ascending k (number of parts). k = 1 is skipped. The
    // largest possible k is n (every part one char). Advance only while the
    // dedupe'd output is still short of the cap, so larger k is never
    // enumerated once a smaller k already fills it.
    let mut k = 2;
    while k <= n && results.len() < config.max_final_candidates {
        let splits = enumerate_splits(&chars, n, k, snapshot, okuri_char);
        if !splits.is_empty() {
            collect_for_k(
                &splits,
                config.max_final_candidates,
                &mut seen,
                &mut results,
            );
        }
        k += 1;
    }

    results
}

/// Enumerate every valid split of the whole yomi into exactly `k` parts.
///
/// Boundaries are chosen left-to-right in the order `prefix_matches` /
/// `okuri_ari_prefix_matches` return readings, which fixes the split
/// enumeration order used as Tiebreak 1. A split is kept only if it tiles the
/// whole yomi and every part has at least one candidate.
fn enumerate_splits<'a>(
    chars: &[char],
    n: usize,
    k: usize,
    snapshot: &'a DictionarySnapshot,
    okuri_char: Option<char>,
) -> Vec<Split<'a>> {
    let mut splits = Vec::new();
    let mut current: Vec<&'a [Candidate]> = Vec::with_capacity(k);
    extend_split(
        chars,
        n,
        k,
        0,
        0,
        snapshot,
        okuri_char,
        &mut current,
        &mut splits,
    );
    splits
}

/// Recursive depth-first split builder. `depth` parts have been fixed so far;
/// the next part starts at `start`. Readings are pushed in dictionary-returned
/// order, so the split list is deterministic and matches Tiebreak 1.
#[allow(clippy::too_many_arguments)]
fn extend_split<'a>(
    chars: &[char],
    n: usize,
    k: usize,
    depth: usize,
    start: usize,
    snapshot: &'a DictionarySnapshot,
    okuri_char: Option<char>,
    current: &mut Vec<&'a [Candidate]>,
    out: &mut Vec<Split<'a>>,
) {
    let is_last = depth + 1 == k;

    if is_last {
        // The last part must consume exactly the remainder of the yomi. Only
        // this part may draw from the okuri-ari bucket; the matches are already
        // in dictionary-returned order (Tiebreak 1).
        let matches = match okuri_char {
            Some(c) => snapshot.okuri_ari_prefix_matches(chars, start, c),
            None => snapshot.prefix_matches(chars, start),
        };
        for m in matches {
            if start + m.length != n {
                continue;
            }
            let cands = match okuri_char {
                Some(_) => snapshot.okuri_ari_candidates(&m.reading),
                None => snapshot.candidates(&m.reading),
            };
            if cands.is_empty() {
                continue;
            }
            current.push(cands);
            out.push(Split {
                parts: current.clone(),
            });
            current.pop();
        }
        return;
    }

    // Non-last parts are always okuri-nashi and must leave at least one char
    // for each of the remaining (k - depth - 1) parts.
    let remaining_parts_after = k - depth - 1;
    for m in snapshot.prefix_matches(chars, start) {
        let next_start = start + m.length;
        if next_start >= n || n - next_start < remaining_parts_after {
            continue;
        }
        let cands = snapshot.candidates(&m.reading);
        if cands.is_empty() {
            continue;
        }
        current.push(cands);
        extend_split(
            chars,
            n,
            k,
            depth + 1,
            next_start,
            snapshot,
            okuri_char,
            current,
            out,
        );
        current.pop();
    }
}

/// Within a fixed `k`, lazily enumerate combinations across all `splits` in the
/// required total order and append distinct output texts to `results` until the
/// cap is hit.
fn collect_for_k(
    splits: &[Split<'_>],
    cap: usize,
    seen: &mut HashSet<String>,
    results: &mut Vec<String>,
) {
    let mut heap: BinaryHeap<Reverse<State>> = BinaryHeap::new();

    // Seed each split with its all-zero (top-of-each-part) combination.
    for (split_order, split) in splits.iter().enumerate() {
        heap.push(Reverse(State {
            rank: 0,
            split_order,
            indices: vec![0; split.parts.len()],
            frontier: 0,
        }));
    }

    while results.len() < cap {
        let Some(Reverse(state)) = heap.pop() else {
            break;
        };
        let split = &splits[state.split_order];

        // Materialize this combination's output text lazily (only on pop).
        let mut text = String::new();
        for (part, &idx) in split.parts.iter().zip(state.indices.iter()) {
            text.push_str(&part[idx].text);
        }
        if seen.insert(text.clone()) {
            results.push(text);
            if results.len() >= cap {
                return;
            }
        }

        // Push successors: bump exactly one index at or after the frontier.
        // Bumping position `p` advances the successor's frontier to `p`, so each
        // reachable tuple is enqueued exactly once.
        for p in state.frontier..split.parts.len() {
            let next = state.indices[p] + 1;
            if next >= split.parts[p].len() {
                continue;
            }
            let mut indices = state.indices.clone();
            indices[p] = next;
            heap.push(Reverse(State {
                rank: state.rank + 1,
                split_order: state.split_order,
                indices,
                frontier: p,
            }));
        }
    }
}
