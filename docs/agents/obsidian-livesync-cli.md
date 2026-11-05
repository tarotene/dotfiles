# obsidian-livesync-cli — vega の LiveSync を公式 CLI の daemon に置き換える運用

実装: `home/modules/obsidian-livesync.nix`(option・ラッパー・unit)、`nix/obsidian-livesync-cli.nix`(CLI の build)
検査: `home/tests/obsidian-livesync.nix`(評価)、`flake.nix` の checks `obsidian-livesync-cli`(build)・`obsidian-livesync`
由来: #1328(置き換えの実装)、#1327(調査。隔離環境で消失・競合なしを確かめた)、#1196(窓が無いと同期しない問題)

Self-hosted LiveSync は Obsidian のプラグインなので、窓が無いと同期せず、vega ではログイン時に
Obsidian を自動起動していた。公式 CLI(`vrtmrz/obsidian-livesync` の `src/apps/cli`)の `daemon` は
窓なしで vault と同期先(CouchDB 互換)を同期する。これを per-user service に置き、Obsidian の
自動起動をやめる。**option の既定は無効**で、有効にするのは本人が初回の手順を済ませたあと。

## 構成

| 部品 | 中身 |
|---|---|
| CLI | タグ `1.0.44-cli` の commit を `buildNpmPackage` でソースから build。出力は `dist/` と実行時の依存 |
| ラッパー `obsidian-livesync` | PATH に出る唯一の入口。サブコマンドは `init` と `daemon` の 2 つだけ(他は終了コード 2) |
| unit `obsidian-livesync.service` | `ExecStart` はラッパーの `daemon`。印か vault が無いと `Assert` で起動しない |
| 印 `~/.local/state/dotfiles/obsidian-livesync/initialized.json` | `init` の成功時にだけ書く。中身は DB の接尾辞(秘密ではない) |
| DB 側ディレクトリ `~/.local/state/dotfiles/obsidian-livesync/db` | CLI の PouchDB と `.livesync/`(vault の外) |
| 設定 | `$XDG_RUNTIME_DIR` の下(tmpfs)にだけ作る。`init` は一時ディレクトリ、`daemon` は unit の `RuntimeDirectory`。停止で消える |

接続情報(LiveSync の設定の JSON)は、Secrets Manager に置いた秘密を、obsidian-backup と同じ経路で
取る: ログインの鍵束の machine token を `BWS_ACCESS_TOKEN` に渡して `bws run --no-inherit-env --`
する。秘密の名前(キー)は公開リポジトリに書けない(ADR-0034)ので、private wrapper から
`dotfiles.obsidian.livesync.secretKey` に渡す。既定は null で、null のとき `init`・`daemon` はエラーになる。

```nix
dotfiles.obsidian.livesync = {
  enable = true;
  secretKey = "<Secrets Manager のキー名>";
};
```

### なぜラッパーを 1 本にするか

#1327 の調査で、次の付け忘れが手元のファイルを消すと分かった。

- 初回の取り出しに `fetch --force --write-settings` を付け忘れる。`daemon` が空の別 DB を開き、全ファイルの
  版を作り直して競合を生む。
- 1.0.44 は DB の接尾辞を設定に保存する。設定を毎回作り直す設計では、接尾辞の無い設定が空の別 DB を開く。

ラッパーは `fetch --force --write-settings` を固定で呼び、接尾辞を印に持ち、`daemon` の設定へ毎回上書きする。
生の CLI は PATH に出さないので、フラグの食い違いはラッパーの外からは書けない。

## 初回の手順(本人が行う。順に 1 つずつ)

初回は、本物の同期先に書く操作(`fetch`)を含むので、本人が 1 回、手で行う。

1. **当日の日誌の競合を、vega の Obsidian の競合画面で解消する。** 解消しないと、新しい取り出しは
   `has conflicts` でこの日誌を飛ばす。CLI の `resolve` は本物の同期先に書くので使わない。
   画面の名前は LiveSync プラグインの表示のままで、この文書では確かめていない(未確認)。
2. 本物の vault と同期先に、前回の試験の痕跡(競合の葉・削除の印)が残っていないかを確かめる(#1327 の
   「未確認」)。見つけたら、取り出す前に片付ける。
3. LiveSync の設定の JSON を、Secrets Manager の、obsidian-backup の machine token が読めるプロジェクトに
   登録する。形式は CLI 上流 README の「Configuration」のとおりプラグインと同じ(`couchDB_URI`・
   `couchDB_USER`・`couchDB_PASSWORD`・`couchDB_DBNAME`・`isConfigured: true`、暗号化を使うなら
   `encrypt`・`passphrase`)。プラグインの設定ファイルをそのまま使えるかは未確認。値は argv に載るので、
   手元の端末で 1 回だけ行い、ファイルに残さない。

   ```bash
   bws secret create "<KEY>" "$(cat <設定の JSON>)" "<PROJECT_ID>"
   ```

   obsidian-backup と同じ machine account が、この秘密も読めるようになる(読み取り専用)。プロジェクトを
   分ける場合は、別の machine token を鍵束に置く経路が要り、この実装は 1 つの token だけを扱う。
4. private wrapper で上の option を有効にし、`hms` で当てる。この時点では印が無いので、unit は
   `Assertion failed` で起動しない(`hms` の degraded と `systemctl --user --failed` に出る。想定内)。

   ```bash
   systemctl --user status obsidian-livesync
   ```
5. Obsidian を終了する(プラグインと daemon を同じ vault に同時に書かせない)。
6. `init` を 1 回実行する。`fetch` は同期先の状態でローカル DB を作り直し、vault へ反映する。

   ```bash
   obsidian-livesync init
   ```

   完了確認: 終了コード 0 で、`initialised (database suffix recorded in …)` と出る。
   `ls ~/.local/state/dotfiles/obsidian-livesync/initialized.json` が存在する。
7. daemon を起動し、読み戻す。

   ```bash
   systemctl --user start obsidian-livesync
   systemctl --user status obsidian-livesync
   journalctl --user -u obsidian-livesync -n 50
   ```

   完了確認: `active (running)`。Obsidian を起動しない状態で、スマホ側の編集が vega の vault に届く
   (#1328 の検証)。

## 挙動と注意

- **`init` は本物の同期先に書く。** `fetch` は同期先の端末の一覧に、この端末を書き足す(#1327 の調査)。
  やり直すときも同じ `init` を実行する(古い印は先に消えるので、失敗した `init` の後に `daemon` は通らない)。
- **mirror は削除を尊重しない**(#1327 の調査)。daemon は起動時に mirror scan を行うので、停止中に片側で
  消したファイルが、起動後に他方から復活しうる。daemon を止めている間は、ファイルを消さない。
- **中断された `fetch` の記録は DB 側ディレクトリに残り、認証情報を含みうる。**
  `~/.local/state/dotfiles/obsidian-livesync/db/.livesync/reconstruction.json`。設定ファイルと同じ扱いで守り、
  共有・バックアップ対象に含めない。記録が残っている間は、通常の同期が止まる。vault・DB・設定を保全し、
  失敗の原因を直してから、同じ `obsidian-livesync init` を再実行する。記録を消して `daemon` を再開しない。
- **vega で Obsidian を手で開くと、プラグインと daemon が同じ vault に書く。** この挙動は未確認。
  Obsidian を開く前に `systemctl --user stop obsidian-livesync` し、閉じてから `start` する。
- **設定の変更**: Secrets Manager の値を書き換え、`systemctl --user restart obsidian-livesync` する
  (`daemon` は起動のたびに取り直す)。DB の作り直しが要る変更(暗号化・パス正規化など)は `init` をやり直す。
- **無効にする**: option を `false` に戻して `hms`。unit は消え、Obsidian の自動起動が戻る。state の
  ディレクトリと Secrets Manager の秘密は残るので、不要なら手で消す。

## 失敗したとき

| 症状 | 見るところ |
|---|---|
| unit が `Assertion failed` | 印が無い(`init` 未実施)か、vault のパスが無い |
| `Bitwarden machine token is missing` | 鍵束が未解錠、または `obsidian-backup configure-token` 未実施(`docs/operations.md` の Obsidian の節) |
| `could not read the secret` | キー名の不一致、machine account がそのプロジェクトを読めない |
| `fetch finished but no database suffix` | `fetch` が接尾辞を設定に書かなかった。印は書かれず、`daemon` は起動しない。journal の CLI の出力を読む |
| `daemon runs only under its systemd unit` | `daemon` を手で実行した。unit 経由(`systemctl --user start`)で起動する |

## 設計の根拠(D12)

- **取得方法**: nixpkgs に該当するパッケージが無く(stable・unstable の検索で 0 件)、release asset にも CLI の
  bundle が無い(plugin の 3 点だけ)ので、ソースからの build を採る。対抗馬の GHCR の OCI イメージ
  (digest 固定)は、実行時の層(podman など)が増えるので採らなかった。
- **PoC の実測**(2026-11-05、x86_64-linux、nodejs_22): `buildNpmPackage` で build が通る。ネイティブ依存の
  leveldown は npm パッケージの `prebuilds/` を使う node-gyp-build 型で、`installCheckPhase` が `--help` と、
  vite が同梱せず外に出す依存(leveldown・pouchdb-adapter-leveldb・pouchdb-adapter-http・chokidar・
  werift)の読み込みを確かめる。出力は約 108 MB。
- **更新**: 上流のタグを上げるときは、`nix/obsidian-livesync-cli.nix` の `rev`・`hash`・`npmDepsHash` を
  上げ、`nix build .#checks.x86_64-linux.obsidian-livesync-cli` で通ることを確かめる。タグでなく commit で
  固定しているのは、タグが動かせるため。
- **未確認**: Obsidian とこの daemon の同時稼働、プラグインの設定ファイルの流用、`fetch` が端末の一覧に
  書き足す挙動の一次ソース(この文書は #1327 の調査に依る)。
