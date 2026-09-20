# ADR-0023 — リポジトリライフサイクル統制(visibility/license/triage/consolidation)を dotfiles に正本化する

- Status: Accepted
- Date: 2026-09-21
- Issue: Closes tarotene/dotfiles#240

## Context

全リポジトリ統廃合 round(grill-me セッション、2026-09-21)の一環で、
`tarotene/dotfiles#240`(別の私設ポートフォリオ管理リポジトリ(PRIVATE)の
ガバナンス文書と ADR-0014/0015/0020(github-audit)の重複・棲み分け確認)を
実行に移した。

その私設リポジトリは、以前の全面統廃合 round(別の私設 time-boxed
リポジトリ(PRIVATE、2026-08-06 完了)で実行)で全リポジトリの
Maintain/Archive/Delete トリアージと
テーマ monorepo 統合を実施した際に、判断規則を `docs/` 配下の 5 文書
(naming / policy / triage / deprecation-checklist / consolidation)として
codify した meta repo である。その後 dotfiles 側で ADR-0014(命名クラス)・
ADR-0015(github-audit 統合監査)・ADR-0020(生成側規則反転)が独立に整備され、
両者の関係が未確認のまま並存していた。

突き合わせの結果:

- **naming.md** は大部分が ADR-0014/ADR-0020 に置換済み。生き残るのは
  「テーマ monorepo(vessel)は研究テーマ名で命名する」規則と、標準
  ディレクトリ構成(`simulations/` `analysis/` `publications/` `notes/`
  `seminars/`)のみ。
- **policy.md**(visibility/license 方針)・**triage.md**(Maintain/Archive/
  Delete 基準 + delete safety net)・**deprecation-checklist.md**
  (deprecate-then-archive の順序)・**consolidation.md**(snapshot+
  PROVENANCE 手順 + 将来の公開標準)の 4 文書は、dotfiles 側に対応する
  機構が一切ない。`github-audit` の 5 ドメイン(rulesets/charters/naming/
  settings/renovate)はいずれも「ガバナンス drift の検査」であり、
  「リポジトリのライフサイクル(生かす/畳む/消す)を判定する」層を
  持たない。
- 同リポジトリの Rust CLI(インベントリ収集・TUI マーキング・Typst
  チェックリスト生成)のうち、収集・drift 面は `github-audit` に機能的に
  置換済み(`docs/github-audit.md` に 2026-09-10 時点の裁定記録が既にある)。
  ライフサイクル判定(dormancy scoring・Maintain/Archive/Delete マーキング)
  面は未置換だが、CI と weekly データリリースが半年近く連続失敗しており、
  自動化としては事実上停止している。

dotfiles は PUBLIC リポジトリであり、私設リポジトリの実名・そこで言及
される private リポジトリの実名は本 ADR にも移設文書にも書かない
(`docs/claude/public-publish-guard.md` の方針)。

## Decision

1. **visibility/license 方針・triage 基準・deprecate-then-archive
   チェックリスト・consolidation 手順(vessel 命名規則を含む)を、
   dotfiles の `docs/repo-lifecycle.md` に正本として移管する。**
   4 文書 + naming.md の生存断片をサニタイズ(private リポジトリ名は
   汎用プレースホルダに置換)した上で 1 つの living doc に統合する
   (ADR-0008 の「腐る事実は living doc、単発の重大判断は ADR」の分離に
   従い、本 ADR は「移管する」という決定そのものだけを持つ)。
2. **`<tech>-playground` の生存/archive 判定は ADR-0020 の閉語彙
   `descriptive-species.tsv` の `lab` 種別語に委ねる**(独立の種別語
   `playground` は追加しない — ADR-0020 Amendment が確立した「主題語では
   なく型を指す器語」の原則に従う)。
3. **Rust CLI は移設しない。** インベントリ・drift 面の機能は
   `github-audit` へ機能的に統合済みで、ライフサイクル判定面は
   コードごと退役させる。ライフサイクル判定(dormancy scoring)を
   `github-audit` に新設するかは別途 Issue で検討する(本 ADR のスコープ外)。
4. **私設リポジトリは、移設内容の検証(このリポジトリでの `nix flake
   check` 通過、内容の逐語確認)後、その repo 自身の deprecation-checklist
   の手順(description → topics → archive)に従って archive する。**
   git 履歴は削除しない(archive は可逆)。

## Alternatives considered

- **現状維持(私設リポジトリを正本のまま残す)** — 判断規則が 2 箇所に
  分裂したまま、drift 検査は `github-audit` に集約されているのに
  ライフサイクル方針だけが到達しにくい private repo に残る非対称を
  放置することになる。棄却。
- **私設リポジトリの docs/ をまるごと dotfiles にコピーする(生存判定を
  省略)** — naming.md の大半は ADR-0014/0020 と矛盾なく重複するだけの
  内容であり、そのまま置くと二重の正本になる。突き合わせて生存断片のみ
  移すほうが ADR-0014/0020 との一貫性を保てる。棄却。
- **Rust CLI のライフサイクル判定機能も dotfiles/github-audit に丸ごと
  移植する** — CLI 自体は 2026-02 以降ほぼ無更新で、移植は新規実装に
  近い作業になる。今回のスコープ(文書の正本化)を超えるため、要否は
  別 Issue で判断する。棄却(この ADR では)。

## Consequences

- `docs/README.md` の Operations 節に `repo-lifecycle.md` を追加する。
- 私設リポジトリは本 ADR の移設 PR が merge された後に archive される
  (統廃合 round の実行手順内、別コミット)。
- 今後の全リポジトリ統廃合 round は `docs/repo-lifecycle.md` の
  triage 基準 + deprecate-then-archive チェックリストを一次基準とする。
- `github-audit` にライフサイクル(dormancy)ドメインを追加するかどうかの
  検討は、follow-up Issue に切り出す。

## Verification

- `nix flake check`(docs のみの変更で活性化パッケージへの影響なし)。
- `docs/repo-lifecycle.md` に private リポジトリの実名が含まれないこと
  (grep で移設元の私設リポジトリ群の実名が出ないことを確認)。
- 移設 PR merge 後、私設リポジトリの archive 実行を目視確認
  (`gh repo view --json isArchived`)。
