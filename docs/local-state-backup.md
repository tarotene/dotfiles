# ローカル状態バックアップと復旧

`local-state-backup` は vega の再生成できないローカル状態を restic で保存する。
source B2 bucket の repository を S3 互換 API で利用し、別アカウントの destination
bucket への Cloud Replication が確認できるときだけ maintenance で prune する。
destination の鍵は vega に置かない。OS、Nix store、Downloads、credential store は対象外。
prune は新しい pack や index を生成し、B2 replication の複製順序は保証されないため、
prune 直後に destination から復旧できるかは未実測である。
異常終了で private state の `local-state-backup/staging` に残骸が残った場合、
次回は停止する。内容に機密データが含まれるため、本人が安全に確認・削除してから再試行する。

## 実行時の設定

private wrapper で `dotfiles.localStateBackup.enable = true;` を vega にだけ指定する。
本モジュールを `home/common.nix` に import し、timer の node を `pipeline-map.toml` に
登録した上で評価する。root での適用や、vega への適用はこの文書の変更だけでは行わない。

BWS の専用 project に次のキーを登録する。値は public repository や Nix store に置かない。
その project の machine token は GNOME Keyring の `application=local-state-backup`、
`credential=bws-access-token` に保存する。machine token と source bucket の権限だけを
vega に置き、destination bucket の credential は別端末に保管する。

source の application key は、restic の読み書きと保持・prune に必要な source bucket の
権限に加え、maintenance の B2 Native API 照会用に `listBuckets` と `listFiles` が必要である。
`listBuckets` は source bucket の ID と名前の照合、`listFiles` は全 object version の
replication 状態の取得に使う。bucket を source に限定できる。どちらかの権限や bucket の
照合結果が得られないとき、maintenance は forget/prune の前に失敗する。
[Backblaze, b2_list_buckets](https://www.backblaze.com/apidocs/b2-list-buckets)
（更新 2026-09-11、取得 2026-10-28）と
[b2_list_file_versions](https://www.backblaze.com/apidocs/b2-list-file-versions)
（更新 2026-09-11、取得 2026-10-28）を参照。

| BWS のキー | 内容 |
| --- | --- |
| `LOCAL_STATE_PROFILE` | 以下の JSON。パス・UUID・bucket ID は機体側の実値に置換する |
| `RESTIC_REPOSITORY` | source bucket の S3 互換 repository URL |
| `RESTIC_PASSWORD` | repository のパスワード |
| `AWS_ACCESS_KEY_ID` | source bucket の application key ID |
| `AWS_SECRET_ACCESS_KEY` | source bucket の application key |
| `HEALTHCHECKS_URL` | 対応する `https://hc-ping.com/…` の URL |

JSON の例は形式だけを示す。`identity` は hostname と無関係な安定 UUID、
`source_bucket_id` は B2 Native API の bucket ID、`source_prefix` は restic repository の
object prefix である。`sources` は存在するものだけを列挙し、linked worktree の共通 Git
領域が repository の外にある場合は、その共通領域も `git-common` で明示する。
`audit_roots` は月次で未登録 repository を探す絶対パスである。

```json
{
  "identity": "00000000-0000-4000-8000-000000000000",
  "source_bucket_id": "EXAMPLE_BUCKET_ID",
  "source_prefix": "example-repository/",
  "sources": [
    {"kind": "git", "path": "/home/example/projects/example"},
    {"kind": "git-common", "path": "/home/example/git-common/example.git"},
    {"kind": "sqlite", "path": "/home/example/.local/share/atuin/history.db"},
    {"kind": "mozc", "path": "/home/example/.config/mozc"},
    {"kind": "jsonl", "path": "/home/example/.local/state/example/events.jsonl"}
  ],
  "excludes": ["*.generated"],
  "audit_roots": ["/home/example/projects"]
}
```

`LOCAL_STATE_PROFILE` は検証後、XDG config の `local-state-backup/profile.json` に mode
0600 で展開される。このファイルと source credential を公開しない。上記のパスは例であり、
既存の実機パスを自動発見したものではない。`inventory` はファイル数とサイズを表示し、
`dry-run` は組込み除外と profile の除外、各対象の filesystem device、restic の dry-run が
選んだファイルを表示する。SQLite・Mozc・JSONL の dry-run は実ファイルを直接検査し、
本番 backup ではそれぞれ整合的な staging を作る。どちらも個人パスを端末に表示するため、
公開ログへ転記しない。

```sh
local-state-backup inventory
local-state-backup dry-run
local-state-backup maintenance --dry-run
local-state-backup audit --verbose
```

日次 backup は停止中に逃した分を次回起動時に実行する。metered 接続や他の実行との
lock 競合などで service が失敗した場合は、どの operation も 1 時間後に再試行する。
月次 check・maintenance・audit と四半期のサンプル restore は、重なりを減らすため発火時刻を分ける。
通常ログは XDG state の
`local-state-backup/local/*.log` に保存し、30 日を超えたファイルを次回実行時に削除する。
状態は `systemctl --user status local-state-backup-backup.service` と
`systemctl --user list-timers 'local-state-backup-*'` で確認する。

Mozc の staging 前に入力サービスが動いていた場合、その状態を XDG state の
`local-state-backup/mozc-input-service-was-active` に記録してから停止する。SIGKILL や
停電で復帰前に終了した場合、次の `local-state-backup` 起動時に BWS へ接続するより先に
サービスの再起動と稼働確認を試み、成功後だけ記録を消す。次の実行まで即座に復旧する
仕組みではなく、実機での停電試験は未実施である。

データと manifest は別の暗号化 snapshot として保存する。データ snapshot には
`local-state`、manifest には `local-state-manifest` と `for-<データ snapshot の完全 ID>` の
タグがある。manifest の保存に失敗すると backup は失敗し、そのデータ snapshot は manifest が
揃うまで maintenance の対象から除外されず、maintenance 自体が中止する。
`latest` 単独は manifest を選ぶことがあるため、データを選ぶときは必ず `--tag local-state` を使う。

## 個別復旧

復元先は稼働中の Git repository や SQLite database に重ねない。別端末で source または
destination の repository と password を使える状態にし、復元後に内容を確認してから
必要な場所へ移す。destination を使う場合も source を変更できない credential を使う。
別端末から password と recovery 手段に到達できるかの確認は本番導入時の作業である。

以下は BWS と `restic`、`jq`、`python3` が端末で使え、BWS project に上記キーがある場合の
手順。source の通常設定を使う場合、まず端末で keyring から token を読み、BWS の secret を
子シェルに渡す。token や環境変数の値は表示しない。

```sh
export BWS_ACCESS_TOKEN="$(secret-tool lookup application local-state-backup credential bws-access-token)"
RECOVERY_COMMAND="$(python3 -c 'import os, shlex; print(" ".join(map(shlex.quote, ["env", "HOME=" + os.environ["HOME"], "PATH=" + os.environ["PATH"], "/bin/sh", "-i"])))')"
bws run --no-inherit-env -- "$RECOVERY_COMMAND"
```

子シェルで、データ snapshot の日時と ID を表示する。`BACKUP_ID` は BWS profile の UUID で、
ここでは `LOCAL_STATE_PROFILE` から読み取る。復元する snapshot の完全 ID を選び、次に入力する。

```sh
BACKUP_ID="$(printf '%s' "$LOCAL_STATE_PROFILE" | jq -r .identity)"
restic snapshots --json --host "$BACKUP_ID" --tag local-state | jq -r '.[] | [.time, .id] | @tsv'
printf 'Restore snapshot ID: '
IFS= read -r BACKUP_SNAPSHOT
MANIFEST="$(restic dump --host "$BACKUP_ID" --tag "for-$BACKUP_SNAPSHOT" latest /local-state-manifest.json)" || exit 1
printf '%s' "$MANIFEST" | jq -r '.sources | to_entries[] | [.key, .value.kind, .value.path] | @tsv'
```

表示した番号から復元対象を選ぶ。次のコマンドは新しい復元先を作り、Git と Git common は
snapshot 内の元のパスから、SQLite・JSONL・Mozc は manifest の staging 対応から復元する。
復元先が既に存在した場合はそこで止まる。

```sh
printf 'Source number: '
IFS= read -r BACKUP_INDEX
SOURCE="$(printf '%s' "$MANIFEST" | jq -er --argjson i "$BACKUP_INDEX" '.sources[$i].path')" || exit 1
KIND="$(printf '%s' "$MANIFEST" | jq -er --argjson i "$BACKUP_INDEX" '.sources[$i].kind')" || exit 1
RESTORE_TARGET="$HOME/restore-local-state-$(date +%Y%m%d-%H%M%S)"
mkdir -m 700 "$RESTORE_TARGET" || exit 1
case "$KIND" in
  git|git-common)
    restic restore "$BACKUP_SNAPSHOT:$SOURCE" --target "$RESTORE_TARGET" --verify || exit 1 ;;
  sqlite|jsonl|mozc)
    STAGED="$(printf '%s' "$MANIFEST" | jq -er --arg p "$SOURCE" '.staging[] | select(.original == $p) | .staged')" || exit 1
    STAGED_PATH="$(restic ls --json "$BACKUP_SNAPSHOT" | jq -er --arg n "$STAGED" 'select(.path? and (.path | endswith("/" + $n))) | .path')" || exit 1
    if [ "$KIND" = mozc ]; then
      restic restore "$BACKUP_SNAPSHOT:$STAGED_PATH" --target "$RESTORE_TARGET" --verify || exit 1
    else
      STAGED_PARENT="${STAGED_PATH%/*}"
      restic restore "$BACKUP_SNAPSHOT:$STAGED_PARENT" --target "$RESTORE_TARGET" --include "$STAGED" --verify || exit 1
    fi ;;
  *) echo 'Unknown source kind' >&2; exit 1 ;;
esac
printf 'Restore directory: %s\n' "$RESTORE_TARGET"
```

linked worktree の場合、上の手順で復元したディレクトリは作業ツリーであり、`.git` ファイルは
元の場所を指している。`git status` を実行する前に、同じ manifest にある対応する
`git-common` の番号を確認し、そのパスを `COMMON_SOURCE` に入力する。以下は復元先が
存在しないことを確認して共通領域を別に復元し、両方向の参照を修復する。元の repository に
重ねず、作業ツリーと共通領域が同じ snapshot に属することを確認する。

```sh
printf 'Git common source number: '
IFS= read -r COMMON_INDEX
COMMON_KIND="$(printf '%s' "$MANIFEST" | jq -er --argjson i "$COMMON_INDEX" '.sources[$i].kind')" || exit 1
[ "$COMMON_KIND" = git-common ] || { echo 'Expected git-common' >&2; exit 1; }
COMMON_SOURCE="$(printf '%s' "$MANIFEST" | jq -er --argjson i "$COMMON_INDEX" '.sources[$i].path')" || exit 1
COMMON_PARENT="$HOME/restore-git-common-$(date +%Y%m%d-%H%M%S)"
mkdir -m 700 "$COMMON_PARENT" || exit 1
restic restore "$BACKUP_SNAPSHOT:$COMMON_SOURCE" --target "$COMMON_PARENT/.git" --verify || exit 1
git -C "$COMMON_PARENT" worktree repair "$RESTORE_TARGET" || exit 1
git -C "$RESTORE_TARGET" rev-parse --git-common-dir
git -C "$RESTORE_TARGET" log -1 --oneline
git -C "$RESTORE_TARGET" status --short
```

`rev-parse` が表示する共通領域が `COMMON_PARENT/.git` を指すこと、履歴と未追跡ファイルが
期待どおりであることを確認する。別の linked worktree も復元した場合は、その復元先を
同じ `worktree repair` に引数として追加する。ここでの対象は common-dir が通常の `.git`
ディレクトリである場合であり、bare repository や submodule の復旧は未検証である。
[Git, git-worktree 2.54.0](https://git-scm.com/docs/git-worktree/2.54.0) の `repair` 仕様
（取得 2026-10-28）に基づく。

表示されたディレクトリで、Git は `git status` と必要な commit・untracked file、SQLite は
`sqlite3 <復元ファイル> 'PRAGMA integrity_check'` が `ok`、JSONL は必要な行、Mozc は
必要なファイルがあることを確認する。確認後に子シェルを `exit` し、親シェルで
`unset BWS_ACCESS_TOKEN RECOVERY_COMMAND` を実行する。

年に一度、予備ディスクまたは VM で bootstrap から復旧を演習し、別 B2 アカウントに新しい
destination bucket を作成して replication を切り替える。新 destination の復元を確認してから
旧 bucket を手動で廃棄する。B2 の replication 完了の判定は
[Backblaze, b2_list_file_versions](https://www.backblaze.com/apidocs/b2-list-file-versions)
（更新 2026-09-11、取得 2026-10-28）、復元は
[restic, Restoring a backup](https://restic.readthedocs.io/en/v0.18.1/050_restore.html)
（取得 2026-10-28）に基づく。
