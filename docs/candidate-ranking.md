# 候補生成ロジック詳細仕様

`CompoundGenerator` が `(yomi, snapshot, config, okuriPrefix)` を入力として候補リストを返すまでの全工程の仕様。README §候補生成のルール の補足ドキュメント。実装本体は [`Sources/skkserv-compound/CompoundGenerator.swift`](../Sources/skkserv-compound/CompoundGenerator.swift)。

このドキュメントは「コードが何をするか」をコードと一対一で写し取ったものです。コードを変えるときはここも同時に変えてください。

## 用語

| 用語 | 意味 |
|---|---|
| **yomi** | 入力読み (例: `せいそうぎょうしゃ`、`minicataloggift`、`もんだいな`)。okuri-ari モードでは送り仮名 ASCII 1 文字を取り除いた残り |
| **okuriPrefix** | 送り仮名 ASCII 1 文字 (`s`, `k` 等)。送りありリクエストでのみ非 nil。サーバーが受信した opcode `1` のオペランド末尾 1 ASCII から抽出 |
| **part / 読み片** | yomi を分割したときの 1 区間。各 part は辞書に登録された見出し語と一致しなければならない |
| **split / 分割** | yomi を `k` 個の part に過不足なく区切る具体的な区切り方 1 通り |
| **k** | split に含まれる part の個数。常に `k ≥ 2` |
| **rank** | ある split の組み合わせ列挙の中で、何番目に pop された組み合わせか (0 始まり)。同 split 内では rank が増えるほど候補順位が下がる |
| **candidate / 候補** | ある reading に対する辞書の漢字表記 1 件。`Candidate(text, source)` で表され、source は `.user` か `.system` |
| **part candidate index** | 1 つの split 内で、その part に使う候補の番号 (0 始まり) |
| **minPartLen** | split に含まれる part の文字数のうち最小値 (okuri-ari の最終 part は送り仮名 ASCII を除いた hiragana stem 長で数える) |
| **enumOrder** | `enumerateRecursive` が split を生成した順番 (0 始まり)。最終的な辞書登録順に依存する決定論的な値 |

## 入出力

入力:

- `yomi: String` — 必須。空または 1 文字の場合は即 `[]` を返す
- `snapshot: DictionarySnapshot` — user / system 辞書をマージ済みのインデックス
- `config: CompoundGeneratorConfig`
  - `maxCandidatesPerReading` (default `5`) — 各 part の候補をこの個数で切る
  - `maxFinalCandidates` (default `10`) — 最終出力候補をこの個数で切る
- `okuriPrefix: String?` — 非 nil なら okuri-ari モード。先頭 1 文字のみを使う

出力: `[String]` — 候補表記の配列。重複なし、配列順位 = 候補順位。

## 全体パイプライン

```
generate(yomi, snapshot, config, okuriPrefix)
  ├─ if |yomi| < 2 → return []
  ├─ for k = 2, 3, …, |yomi|:
  │    splits ← enumerateSplits(k, yomi, snapshot, …)
  │    expand(splits, finalCap, &seen, &result)
  │    if |result| ≥ finalCap → break
  └─ return result
```

外側の `k` ループは「語数が少ない分割を強く優先する」ためのもの。`k = 2` の出力で `maxFinalCandidates` が埋まれば `k ≥ 3` の split は一切列挙されない (cf. `skipsHigherKOnceFinalCapFilled`)。

## Split 列挙 (`enumerateSplits` / `enumerateRecursive`)

与えられた `k` に対し、yomi を `k` 区間に過不足なく区切る split を全列挙する。

### アルゴリズム

各深さで現在位置 `start` から始まる prefix match (`snapshot.prefixMatches`) を列挙し、残り長と残り part 数の整合 (`(n - start) >= remainingParts`) を取りながら DFS で深さ `k` まで掘る。深さ `k` に達し `start == n` ならその split を採用。

```
enumerateRecursive(k, depth, start, chars, …, parts):
  if depth == k:
    if start == n: emit makeSplit(parts)
    return
  if (n - start) < (k - depth): return
  isLast ← (depth == k - 1)
  matches ← isLast かつ okuriPrefix あり
              ? snapshot.okuriAriPrefixMatches(chars, start, okuriPrefix)
              : snapshot.prefixMatches(chars, start)
  for (length, reading) in matches:
    nextStart ← start + length
    if isLast:
      if nextStart ≠ n: continue              # 最終 part は body 全消費
    else:
      if (n - nextStart) < (k - depth - 1): continue
    parts.push(reading); recurse; parts.pop()
```

### `prefixMatches` の挙動 (cf. `DictionarySnapshot.prefixMatches`)

`snapshot.readingsByFirstCharacter[chars[start]]` を辞書登録順に走査し、prefix 一致した reading を「登録順」でそのまま並べる。長短ソートはしない。したがって `enumOrder` は **辞書登録順** に決まる決定論的な値で、長さ順ではない。

### okuri-ari モードの制約

- okuriPrefix が非 nil の場合、**最終 part のみ** が okuri-ari バケット (`snapshot.okuriAriPrefixMatches`) を引く。中間 part は通常通り okuri-nashi バケットを引く。
- okuri-ari の reading は `なs` のように末尾 ASCII を含む形で辞書に登録されているが、prefix match は「ASCII を除いた hiragana stem」と yomi を照合する。最終 part の reading は `なs` のまま、yomi 消費長は `1` (= `な`) になる。
- 1 部品で yomi 全部を消費する split (= 単独 okuri-ari エントリの完全一致) は `k ≥ 2` 制約により出てこない (cf. `skipsOkuriAriSingleWordExactMatch`)。
- 中間 part に okuri-ari reading を使う split は禁止 (cf. `okuriAriOnlyAtLastPart`)。
- `okuriPrefix == nil` のときは最終 part も通常通り okuri-nashi バケットを引き、okuri-ari バケットには触れない (cf. `okuriPrefixNilFallsBackToOkuriNashi`)。

### `makeSplit` での post-process

採用された split は次の正規化を経て `SplitInfo` になる:

1. **partLens 計算**: 各 part の reading 文字数。okuri-ari の最終 part は `count - 1` (ASCII 1 文字を除外)。これにより「同じ body 長の okuri-nashi split と minPartLen 比較で対等に競える」。
2. **minPartLen**: 上記 partLens の最小値。
3. **part candidates 取得**: 各 part について `snapshot.candidates(for: reading)` (最終 part のみ okuri-ari なら `okuriAriCandidates`) を引き、先頭 `maxCandidatesPerReading` 件で切る。
4. **空 part のチェック**: いずれかの part の候補が空なら `nil` を返し split として採用しない。

`SplitInfo` は `{partCandidates, numParts, minPartLen, enumOrder}` を保持する。

## Expand (組み合わせ展開と順位付け)

### 入力

- `splits: [SplitInfo]` — 同じ `k` を持つ split 全件
- `finalCap: Int` — `maxFinalCandidates`
- `seen: inout Set<String>` — 既出候補テキスト (k ループをまたいで共有)
- `result: inout [String]` — 出力列 (k ループをまたいで共有)

### Pre-sort

入力 `splits` を `compareSplitKey` で安定ソートする。比較順:

1. `minPartLen` 降順 (大きい方が先)
2. `enumOrder` 昇順 (小さい方が先)

ソート後の配列インデックス `splitIdx` が、以下の heap 比較で決定論的フォールバックとして機能する。

### Heap で組み合わせを展開

各 split から「次に取り出すべき候補組み合わせ」を 1 件ずつ min-heap に乗せ、毎回最優先エントリを pop する。

**Heap エントリ** `PQEntry = {splitIdx, rank, indices, text}`:

- `splitIdx` — pre-sort 後のインデックス
- `rank` — その split から pop された回数 (= この組み合わせがその split で何番目に出るか、0 始まり)
- `indices` — 各 part について現在使う候補番号の組
- `text` — `indices` から組み立て済みの候補文字列

**Heap 優先度** (上から強い順):

1. **`minPartLen` 降順** — 大きい split がまず尽きるまで pop される (round-robin より上位)
2. **`rank` 昇順** — 同じ `minPartLen` 帯の中では、すべての split の rank-0 を先に並べ、続けて rank-1 を並べる (round-robin)
3. **`splitIdx` 昇順** — 同 `minPartLen` 同 `rank` での決定論的フォールバック (= pre-sort で encode された `enumOrder`)

**初期化**: 各 split について `indices = (0, 0, …, 0)`, `rank = 0` のエントリを 1 つずつ push。

**ループ本体**:

```
while |result| < finalCap:
  entry ← heap.pop()
  if entry == nil: break
  if entry.text ∉ seen:
    seen.insert(entry.text); result.append(entry.text)
    if |result| ≥ finalCap: return
  next ← entry.indices をインクリメント (最右が最速で進む lex 順)
  if next が範囲内に収まる:
    heap.push({splitIdx: entry.splitIdx, rank: entry.rank + 1, indices: next, text: 組立})
```

**indices のインクリメント**: 最右の index を `+1`、`partCandidates[i].count` に達したら `0` に戻して左隣を `+1`。最左で繰り上がった (`i < 0`) 場合はその split を使い切ったので heap への再投入はしない。

### Dedupe (重複除去) ルール

`seen` セットで候補テキスト単位の重複を除く。優先度の高い split で先に出た候補が残り、後続の split から同じテキストが来たら捨てる (cf. `dedupesCandidates`)。

`seen` と `result` は外側の `k` ループをまたいで共有されるので、`k = 2` で出た候補が `k = 3` で再度生成されても 1 回しか出力されない (cf. `bestFirstRetreatsToLowerSplitWhenNeeded`)。

## 順位付けの全体プロファイル

最終出力での候補順位を支配するキーは、強い順に:

1. **`k` 昇順** — 外側ループが小さい `k` を先に処理する
2. **`minPartLen` 降順** — 同じ `k` の中では最短 part が長い split が先
3. **`rank` 昇順** — 同じ `(k, minPartLen)` 帯では各 split の rank-0 を先に並べ、続けて rank-1 を並べる (round-robin)
4. **`splitIdx`/`enumOrder` 昇順** — 同 rank の中では辞書登録順で先に登録されている split が先
5. **Part candidate indices** — 同じ split 内の組み合わせは lex 順 (最右最速)。各 part 内の候補順は `snapshot.candidates(for:)` の順 (= user 候補が先、続いて system 辞書を CLI 指定順)

## キャップの作用

- **`maxCandidatesPerReading`**: 各 part について `snapshot.candidates(for: reading)` の先頭 N 件で切る。`makeSplit` で 1 回適用される。下位候補は完全に切り捨てられ、heap には登場しない
- **`maxFinalCandidates`**: 最終 `result` の長さ上限。`expand` ループの先頭と末尾の両方でチェックする。外側 `k` ループの継続条件にもなる

両キャップを共に大きくすれば候補は出尽くすが、計算量は組み合わせ数 (∏ partCandidates.count) と split 数の積に比例。

## 仕様で意図的に扱わない事項

- **1 語完全一致は返さない** — `k = 1` を列挙しない。SKK クライアントの通常辞書検索が出すべき責務 (cf. `skipsSingleWordExactMatch`)
- **意味的フィルタなし** — 「単純蚊」のような不自然な候補も生成される可能性がある。順位とキャップで実用上見えにくくし、user 辞書での学習に委ねる
- **送り仮名 hiragana の付加なし** — okuri-ari モードでも候補は連結漢字部分のみ。SKK クライアントが補う
- **活用変換なし** — part の候補表記をそのまま結合
- **補完 (opcode `4`) なし** — 常に `4\n`

## 既存テストとの対応

| 仕様項目 | テスト |
|---|---|
| 基本連結 | `combinesSeisouGyousha`, `combinesTanjunka` |
| 1 語完全一致除外 | `skipsSingleWordExactMatch`, `skipsOkuriAriSingleWordExactMatch` |
| k 昇順優先 | `prefersFewerParts`, `twoPartBeatsThreePartAcrossK`, `skipsHigherKOnceFinalCapFilled` |
| `minPartLen` 降順 | `prefersLongerMinPart` |
| 同 `minPartLen` 帯の round-robin | `roundRobinsBetweenSplitsWithSameMinPartLen`, `roundRobinsZenkengen` |
| `maxCandidatesPerReading` | `respectsPerReadingCap`, `okuriAriRespectsPerReadingCap` |
| `maxFinalCandidates` | `respectsFinalCap`, `bestFirstSmallCapStaysOnTopSplit` |
| Dedupe | `dedupesCandidates`, `bestFirstRetreatsToLowerSplitWhenNeeded` |
| abbrev (全 ASCII) | `combinesAbbrevKatakana`, `combinesAbbrevItemCardSet`, `combinesFourPartAbbrev` |
| okuri-ari 基本 | `combinesOkuriAriCompound` |
| okuri-ari 中間 part 禁止 | `okuriAriOnlyAtLastPart` |
| okuri-ari モードでの送りなし split 除外 | `okuriPrefixDoesNotEmitOkuriNashiSplits` |
| `okuriPrefix == nil` での fallback | `okuriPrefixNilFallsBackToOkuriNashi` |
| `k` 上限 | `skipsKLargerThanYomiLength` |
| 短い読みの許容 | `allowsShortReadings` |
