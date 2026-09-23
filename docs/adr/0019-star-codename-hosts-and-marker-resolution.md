# ADR-0019 — 恒星コードネームのホスト命名と論理ホスト名のマーカー解決

- Status: Accepted
- Date: 2026-09-19
- Issue: No-Issue(grill-me セッション中に発見・裁定)

## Context

既存 3 ホストは `<identity>-pop[-<generation>]`(`personal-pop` /
`company-pop-old` / `company-pop-new`、#207 由来)という役割ベースの命名で、
かつ `homeConfigurations` のキーは OS の `hostname` と一致していなければ
ならない(`home-manager switch --flake .#$(hostname)` が自動選択する
前提、`scripts/hms.sh` / `bootstrap.sh` も同様)。

M2 MacBook Air 向けに新しいホスト(altair)を追加するにあたり、この二点が
両方とも脆さの原因になった:

1. **役割ベース命名は長期的に事実と乖離する。** `company-pop-old` /
   `company-pop-new` という generation 接尾辞は、次に会社支給機が更新
   されるたびに全部繰り上げるか、意味不明な番号だけが増えていく。
   identity(personal/company)をホスト名に刻む設計自体も、ホストの
   identity は `home/hosts/*.nix` が import する identity モジュールで
   既に宣言的に決まっており、ホスト名への重複表現でしかない。
2. **`homeConfigurations` キー = OS hostname という結合は移植性がない。**
   macOS の `hostname` は `Something.local` 的な値になりがちで、かつ
   `scutil --set HostName` で人間が手で設定するものであり、flake の論理
   キーとして安定した入力ではない。

## Decision

1. **新規ホストは恒星名のコードネームを使う。** 役割・identity・世代を
   名前に埋め込まない(RFC 1178 の助言に従う — 下記参照)。新 Mac は
   **altair**(MacBook *Air* との語呂を兼ねる)。既存 3 ホストの改名は
   本 ADR のスコープに含めない(→ 後述の Issue)。
2. **論理ホスト名の解決を「マーカーファイル優先、`hostname` フォール
   バック」にする。** パスは `${XDG_CONFIG_HOME:-$HOME/.config}/dotfiles/host`、
   フォーマットは論理ホスト名 1 行。`scripts/hms.sh` と `bootstrap.sh` の
   両方に同一ロジックの `resolve_host()` を複製導入する(両者は独立配布
   物であり、共有ライブラリ化はしない)。マーカーが存在しない・空の場合
   は従来通り `hostname` にフォールバックするため、**既存 3 ホストは
   無改修で動き続ける**。
3. **マーカーはホストモジュール自身が宣言的に正本を持つ。** 例えば
   altair.nix は `xdg.configFile."dotfiles/host".text = "altair\n"` で
   自分自身のマーカーを配備する。ブートストラップ手順ではこの配備が
   効くまでの間だけ、セットアップ手順書の指示で先に手置きする
   (`echo altair > ~/.config/dotfiles/host`)。この手置きファイルと
   home-manager が配備するファイルの内容は完全一致するとは限らない
   (改行の有無など)ため、初回 activation の衝突回避は内容一致ではなく
   `HOME_MANAGER_BACKUP_EXT=backup`(`home-manager switch -b backup` の
   実体、`bootstrap.sh` の直接 `activate` 呼び出しに付与)に委ねる。

## 先行例

RFC 1178 "Choosing a Name for Your Computer"(D. Libes, 1990)
<https://www.rfc-editor.org/rfc/rfc1178>(取得 2026-09-19)—
「役割やモデル名をホスト名に埋め込むな、そのホストの役割は変わりうる」
という趣旨の助言が明文化されている。本 ADR の恒星コードネーム化は、この
助言をそのまま適用したもの。

マーカーファイルによる論理ホスト名解決そのものについては、確立された
先行パターンを見つけられなかった(探した範囲: home-manager 公式 docs、
コミュニティの flake 構成例)。home-manager コミュニティの一般解は
`--flake .#<name>` を明示指定することであり、hostname 非依存の自動解決を
提供する確立された機構は存在しない。本 ADR のマーカーは、その明示指定を
「一度書けば忘れられる」薄い永続化として設計した、本リポジトリ固有の
糖衣である。

## Alternatives considered

- **`scutil --set HostName` で macOS 側の hostname を人間が明示的に恒星名
  へ設定し、`$(hostname)` 自動選択の仕組みを変えない** — 機構は不変で
  シンプルだが、「hostname に刺さるのが面倒」というそもそもの動機
  (グリルセッションでのユーザーの要求)を解消しない。棄却。
- **環境変数(`DOTFILES_HOST`)で解決する** — シェル起動前に評価される
  `bootstrap.sh`/`hms.sh` の実行コンテキストでは、どの rc ファイルが先に
  読まれるかという鶏卵問題があり、マーカーファイルより壊れやすい。棄却。
- **既存 3 ホストも今回まとめて恒星名へ移行する** — 各ホストでの
  再 apply が必要になり、作業量が今回のタスクの本旨(altair の新設)から
  数段跳ね上がる。ユーザーとの合意によりスコープ外とし、移行専用の
  Issue に切り出す。

## Consequences

- 新規ホストのモジュールは恒星名で追加していく。既存 3 ホストは当面
  `<identity>-pop[-<generation>]` のまま残り、`homeConfigurations` に
  新旧の命名規則が混在する過渡期が生じる。
- 既存 3 ホストの恒星コードネームへの移行は別 Issue(本 ADR 採用後に
  起票)で扱う。
- `scripts/hms.sh` / `bootstrap.sh` の `resolve_host()` は、将来
  マーカーファイルの正本を持たない(=まだ移行していない)ホストでも
  安全に動く必要がある — フォールバックを外してはならない。

## Verification

- `bash -n scripts/hms.sh bootstrap.sh` および `shellcheck -S error` が通る。
- 既存 3 ホストの `nix build .#homeConfigurations.<host>.activationPackage`
  の store path が、`resolve_host()` 導入前後で変化しない
  (ホスト解決はモジュール評価に影響しないため当然だが、回帰確認として
  実施)。
- 実機(altair)での検証はセットアップ手順書(`docs/setup-macos.md`)の
  検証節に記載: マーカー設置 → `bootstrap.sh` → `hms` が `altair` を
  正しく解決すること。

## Amendment (2026-09-23 — #214)

wrap-up chores セッション中、命名語彙が単一ホスト(altair)だけを念頭に
置いており、既存 3 ホストへの改名(当時「別 Issue で扱う」としていた
もの)と将来の自宅サーバー増設の両方を見据えた語彙の閉じ方を明示して
いなかったことが分かった。本 Amendment はこれを確定させる。

1. **命名語彙は「IAU が公認した恒星固有名」の単一フラット空間とする。**
   天体クラス(恒星=作業機 / 惑星・衛星=サーバー等)で用途ごとに語彙を
   分けるアプローチは、ADR-0019 Decision 1 の「役割・identity・世代を
   名前に埋め込まない」という原則そのものと矛盾するため採らない。
   IAU 公認固有名はおよそ 450 件あり実質枯渇しない — 自宅サーバー等
   将来のホスト増設にもこの単一空間からそのまま採番できる。
2. **既存 3 ホストの改名を完了する**(#214、本 ADR が「別 Issue」と
   していたもの): `personal-pop` → `vega`、`company-pop-new` →
   `arcturus`。両者とも IAU 公認固有名。
3. **`company-pop-old` は改名せず削除する。** 既に退役済みのホストを
   新しい名前空間に持ち込む意味がないため(死んだ機構は今消す方が
   後で消すより安い、docs/adr/0035-selection-grounding.md)。
   `home/hosts/company-pop-old.nix` を削除し、対応する
   `homeConfigurations` キーも即削除する(移行期間の旧キーエイリアスは
   改名した 2 ホストにのみ設ける)。
