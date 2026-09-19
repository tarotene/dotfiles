# ADR-0014 — リポジトリ命名クラス体系を定め、GitHub topics を正本にする

- Status: Accepted
- Date: 2026-09-18
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

個人アカウント(`tarotene`)の全 34 リポジトリの命名を実地調査した結果、
文字種・区切り記号の規約(小文字 ASCII kebab)は既に全件満たされており、
形式チェックだけを機械化しても何も検出しない。痛みの実体は形式ではなく
**分類体系の不在**だった — `pj-` prefix 付きの時限プロジェクト型、
`telepath` のような一語コードネーム型、`publish-guard` のような記述的
複合語型、FQDN 形のドメイン名サイト型が、命名時に従うべき規則なしに
混在していた(具体的にどのリポジトリがどう混在していたかは
`docs/claude/public-publish-guard.md` の方針により本 ADR には書かない —
private/company リポジトリ名は dotfiles の成果物に残さない)。加えて
`pj-` prefix の付与漏れ(時限プロジェクトなのに prefix がない、または
その逆)のような、名前とクラスの不一致は名前だけを見ても機械的には
判定できず、意味判断が要る。

## Decision

1. **命名クラスを 4 種に固定する。**
   - `naming-codename`: 恒久的なツール・単一責務の実装。一語(例: `telepath`)。
   - `naming-descriptive`: 研究・記録・コンテンツ系の複合語。kebab-case の
     複合語(例: `publish-guard`)。
   - `naming-pj`: 期限のある個別プロジェクト。`pj-` prefix 固定
     (例: `pj-<project-name>`)。
   - `naming-site`: 公開ドメインに対応するサイト。FQDN 形
     (例: `<name>.example`)。
2. **正本は GitHub topics に置く。** 各リポジトリに上記 4 種のいずれか
   1 つを topic として宣言する(`naming-pj` 等)。README や CONTEXT.md の
   ような git 管理下のファイルに書かない — topics は API から機械的に
   読め、Web UI でも即座に見える。
3. **形式は機械判定、クラス帰属は LLM ノードが人間に委ねる。** 「名前が
   宣言済みクラスのパターンに一致するか」(`naming-pj` なら
   `^pj-[a-z0-9-]+$` 等)は `github-audit` の naming ドメインが決定的に
   判定する(ADR-0015)。「このリポジトリはどのクラスに属すべきか」という
   意味判断は、監査が検出した `class-undeclared`(topic 0 個)・
   `class-ambiguous`(topic 2 個以上)・パターン不一致の findings を
   入力に、LLM ノード(`github-audit-triage` スキル)が提案し人間が裁定
   する。
4. **既存リポジトリのクラス裁定は本 ADR の対象外。** `.github` は GitHub
   予約名として恒久 exempt。`*-inventory` 群・`*files` 群のような帰属が
   自明でないリポジトリのクラス裁定・改名は、別セッションの棚卸しで
   `github-audit-triage` を使って行う。

## Alternatives considered

- **形式(文字種・区切り)だけを監査する** — 実データで全件が既に適合して
  おり、一貫性の痛みの実体(分類体系の不在)を検出できない。棄却。
- **命名クラスを README や CONTEXT.md に書く** — 機械可読性が API 経由で
  劣り、ADR-0016 が禁じるルート文書の肥大化(CONTEXT.md の解体)とも矛盾
  する。棄却。
- **クラス帰属も含めて全自動で機械判定する** — 「この名前が実際の責務を
  正しく表しているか」は意味判断であり、決定論のみでは判定できない
  (ADR-0012 と同じ「形式は機械、内容は LLM」の原則)。棄却。

## Consequences

- `github-audit` の naming ドメイン(ADR-0015 実装)が本 ADR のクラス語彙
  とパターンを判定基準として参照する。
- `repo-charter` スキルの作成時インタビューに命名クラス質問と
  `naming-*` topic 播種の手順が加わる(ADR-0016 実装)。
- 本 ADR の時点では、全 34 リポジトリが `class-undeclared`(topic 未設定)
  と判定される。これは想定内の初期状態であり、クラス裁定自体は別セッ
  ションの棚卸しで行う。
- 命名クラスの語彙・パターンを変更する場合は、この ADR を supersede する
  新しい ADR を起こす(ADR-0008 の規約)。

## Verification

- `github-audit naming --selftest` — `class-undeclared` /
  `class-ambiguous` / パターン一致 / パターン不一致の分岐を fixture で
  確認。
- `github-audit naming --json` を実アカウントに対して実行し、全リポジト
  リが `class-undeclared` と報告されることを確認(2026-09-18 時点)。
