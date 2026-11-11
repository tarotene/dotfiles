# ADR-1237 — CachyOS への移行(#249)を保留し、固まりの記録で再訪の条件を確かめる

- Status: Accepted
- Date: 2026-10-28
- Issue: No-Issue(#249 は保留の記録として open のまま。再訪の条件が成立したら、この ADR を改訂する)

## Context

#249 は、全ホストを COSMIC Desktop on CachyOS へ段階的に移行する構想で、動機は 3 つ: 性能(カーネルのチューニング、x86-64-v3 ビルド)、
AUR/pacman のエコシステム、学習。vega の「GUI が数秒固まる」が、性能の動機の中心だった。

2026-10-26 の分析(#249 のコメント)は、CachyOS の各要素を症状に当てて、「効くとは言えない」と結論した。効きそうな部分の多くは
Pop!_OS のままで得られる、という内容である(同コメントに出典と取得日。本線では再確認していない)。そのうえで、前提が 2 つ変わった。

- vega は 2026-10-27 02:54 に `apt full-upgrade` した(`/var/log/apt/history.log`、読んだ)。kernel は `7.2.6-76070206-generic`(読んだ)。
  分析の「更新待ち 1141 件」「kernel 6.17.4」は過去の値になった。
- 症状の原因の一つとされた system76-scheduler の nice 12 は、#1056 の drop-in で nice 0 に直した(`docs/operations.md`)。

2026-10-28 の #249 の追記には、会社用の EDR の installer が Pop!_OS 24.04 の deb を前提にしていること、CachyOS での会社の許容と
センサーの対応は未確認であること、fcitx5 の daemon を home-manager で持つ構成が Arch でも成り立つかは未検証であることが書かれている
(Arch Linux・CrowdStrike の公開ページの引用つき。本線では未再読)。

## Decision

**移行を保留する。** 動機のうち性能は、更新後の実測が出るまで根拠にしない。AUR と学習だけを理由にした移行は、今は選ばない。
`home-manager` が user 環境の単一正本である三層モデル(ADR-0001)は、移行しても変わらない。

再訪する条件は、次の 3 つのどれかが成立したときとする。

1. 更新済みの状態(`apt full-upgrade` 後、nice 0 の drop-in 後)でも、固まりがビルド中などの CPU の負荷が高い区間に重なって残る。
   (固まりが PSI の CPU の圧力の高い区間に重なる → まず scheduler の優先度を直し、それでも残れば BORE を試す根拠になる。
   固まりの間も PSI が低く cosmic-comp の CPU が跳ねる → 合成器の問題で、上流に報告する。#249 の判定表のとおり。)
2. apt と nix のどちらにも無く、AUR にしか無い必要なパッケージが出る。
3. 会社の EDR の提供元が、Arch 系(CachyOS)での動作を明言する。

①を確かめるために、固まりの記録を取る。道具は、#249 の手順が指名している **atop**(nixos-26.05 の 2.12.1)の raw 記録を、vega の
user service として回す。自前の timer で `/proc/pressure/*` を記録する案は、atop の raw ファイルが PSI を持つので採らない。

### 実測(2026-10-28、vega、一般ユーザー、使い捨ての `HOME`)

- `atop -w <file> 5` が root 無しで動き、`atop -r <file> -P PSI` に CPU・メモリ・IO の圧力の行が出る(avg10・avg60・avg300・累積)。
- 1 サンプルあたり約 8 KB(31 サンプル = 261 KB)。10 秒間隔で 1 日 約 65 MB。3 日分を残して消す。
- root が要るのは短命なプロセスのアカウンティング(`atopacctd`)で、持たない。固まりの前後の区間を見るには足りる、という判断(未検証)。

### 採らなかった案

- apt と pacman の対応表と、漏れを検出する CI: 移行を決めた後なら価値があるが、system-manager でシステム層を宣言する #1228 と方向がぶつかる。
- 手で atop を回す(Issue の手順のまま): 記録を取り忘れる。service にして、取り忘れを無くす。

## Consequences

- #249 は open のまま、現在の合意は「保留」。条件①〜③が成立したら、この ADR を改訂して、移行するか見送りを確定する。
- vega は atop の raw ファイルを `~/.local/state/atop/atop_YYYYMMDD` に溜める(3 日分、約 200 MB)。固まったら `logger -t freeze-mark …` で印を付けて、
  `docs/operations.md` の「Freeze trace」で該当区間を開く。
- PSI の値の読みを、固まりと突き合わせる作業は人の判断で、記録が 1〜2 日溜まってから行う。

## 執行点

- `home/hosts/vega/atop-trace.nix`
- `home/hosts/vega.nix`
- `pipeline-map.toml`

## Amendment(2026-11-11): #249 を閉じ、この ADR を保留の正本にする

#249(CachyOS への移行構想の Issue)は閉じた。保留の現在の合意、再訪の条件①〜③、固まりの判定表は、この ADR が正本である
(ADR-1529 D1 の置き場 4「保留の裁定・再訪の条件」)。上の「Issue:」行と Consequences の「#249 は open のまま」は、この節で
置き換える。#249 の本文とコメント(2026-10-26 の分析、2026-10-28 の再検討)は、経緯と出典の記録として読める。

- 残る作業: 条件①の判定は人が行う。atop の記録と `freeze-mark` の印を、PSI の CPU の圧力と突き合わせる作業である。2026-11-11 の
  時点で、判定の結果は Issue と ADR のどこにも見つからない。固まりが起きて印を付けたら、`docs/operations.md` の「Freeze trace」の
  手順で該当の区間を読み、結果をこの ADR に追記する。条件のどれかが成立したら、この ADR を改訂して、移行するか見送るかを確定する。
  移行を採ると決めたら、そのときに新しい Issue を起こす。
- 未確認のまま残す事項(再訪したときの調査の起点。詳細と出典は #249 の本文):
  - 会社用 EDR が Arch 系に対応しているか。会社が CachyOS を許容するか。
  - fcitx5 の daemon を home-manager で持つ構成が、Arch の `fcitx5-qt` と共存できるか。
  - システム層の apt パッケージに対応する pacman の候補(`base-devel`・`systemd-libs`・`pkgconf`・`arm-none-eabi-gcc`/
    `arm-none-eabi-newlib`・`gnupg` に含まれる `scdaemon`・`tailscale`・`ufw`)が、ABI と起動の面で使えるか。
  - bootstrap の CachyOS 対応。今の `bootstrap.sh` は CachyOS を検出すると、何も変えずに止まる。
  - CI の apt `--dry-run` に相当する、pacman 側の検査。
- 範囲外: nix 閉包の x86-64-v3 ソースビルドと自前のバイナリキャッシュは、この構想とは別の検討項目で、起票していない。
- 執行点の訂正: `home/modules/atop-trace.nix` は `home/hosts/vega/atop-trace.nix` に移った(`pipeline-map.toml` の `atop-trace`
  ノードの `entity`)。上の「執行点」の節を直した。
