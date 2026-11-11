# vega の Self-hosted LiveSync を、Obsidian のプラグインから公式 CLI の daemon
# (per-user service)に置き換える(#1328)。既定は無効。
#
# 入口は `obsidian-livesync` の 1 本のラッパーで、サブコマンドは閉じた 2 つだけ:
#   init    接続情報(設定の JSON)を Secrets Manager から取り、tmpfs の設定で
#           `fetch --force --write-settings` を固定で呼ぶ。成功したら、取り出しで選ばれた
#           DB の接尾辞と「初期化済み」の印を state に書く。本物の同期先に書く操作を
#           含むので、本人が 1 回、手で実行する。
#   daemon  印と vault があるときだけ、設定を tmpfs に作り直し、印の接尾辞を上書きして
#           CLI の daemon を exec する。unit が呼ぶ。
# 生の CLI は PATH に出さない。`fetch --force --write-settings` の付け忘れと、設定を毎回
# 作り直すことで空の別 DB を開く事故(上流 1.0.44 の `main.ts` が接尾辞を設定に保存する
# 仕様)は、ラッパーの外からは書けない(#1327)。
#
# 接続情報の経路は obsidian-backup と同じ: ログインの鍵束(`application obsidian-backup
# credential bws-access-token`)の machine token を `BWS_ACCESS_TOKEN` に渡して
# `bws run --no-inherit-env --` する(crates/obsidian-backup/src/main.rs の `with_bws`)。
# 秘密の名前(Secrets Manager のキー)は公開リポジトリに書けない(ADR-0034)ので、
# option で渡し、既定は null(null のとき `init`・`daemon` はエラーにする)。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.obsidian.livesync;
  vaultPath = config.dotfiles.obsidian.vaultPath;
  home = config.home.homeDirectory;

  stateDir = "${home}/.local/state/dotfiles/obsidian-livesync";
  # CLI の DB 側ディレクトリ(`.livesync/` に DB と、中断された fetch の記録を持つ。
  # 記録は認証情報を含みうるので、設定ファイルと同じ扱いにする)。vault とは別に置く。
  dbDir = "${stateDir}/db";
  # 「初期化済み」の印。`init` が成功したときだけ書く。中身は DB の接尾辞(秘密ではない)。
  mark = "${stateDir}/initialized.json";

  wrapper = pkgs.writeShellApplication {
    name = "obsidian-livesync";
    runtimeInputs = [
      pkgs.coreutils
      pkgs.jq
      pkgs.bws
      pkgs.libsecret
    ];
    text = ''
      umask 077

      cli=${lib.getExe pkgs.obsidian-livesync-cli}
      vault=${lib.escapeShellArg (toString vaultPath)}
      state=${lib.escapeShellArg stateDir}
      db=${lib.escapeShellArg dbDir}
      mark=${lib.escapeShellArg mark}
      secret_key=${lib.escapeShellArg (if cfg.secretKey == null then "" else cfg.secretKey)}

      die() {
        printf 'obsidian-livesync: %s\n' "$*" >&2
        exit 1
      }

      usage() {
        printf 'usage: obsidian-livesync init | daemon\n' >&2
        exit 2
      }

      [ "$#" -eq 1 ] || usage

      # 接続情報(設定の JSON)を Secrets Manager から取り、$1 に書く($1 は tmpfs の中)。
      fetch_settings() {
        local dest=$1 token
        [ -n "$secret_key" ] || die "dotfiles.obsidian.livesync.secretKey is null; set the Secrets Manager key name"
        token=$(secret-tool lookup application obsidian-backup credential bws-access-token) \
          || die "Bitwarden machine token is missing from the login keyring"
        [ -n "$token" ] || die "Bitwarden machine token is empty"
        BWS_ACCESS_TOKEN=$token bws run --no-inherit-env -- ${pkgs.coreutils}/bin/printenv "$secret_key" >"$dest" \
          || die "could not read the secret from Secrets Manager (key: $secret_key)"
        # jq の失敗は、入力の断片を出しうるので、標準エラーを捨てる。
        jq -e 'type == "object"' "$dest" >/dev/null 2>&1 \
          || die "the secret is not a JSON object (expected the LiveSync settings JSON)"
      }

      # 設定の中の DB の接尾辞。空・欠落のときは、古い DB を指してしまうので失敗させる。
      suffix_of() {
        jq -er '.additionalSuffixOfDatabaseName | select(type == "string" and length > 0)' "$1"
      }

      init() {
        local work suffix
        [ -d "$vault" ] || die "vault directory does not exist: $vault"
        [ -n "''${XDG_RUNTIME_DIR:-}" ] || die "XDG_RUNTIME_DIR is not set (the settings must live on tmpfs)"
        work=$(mktemp -d "$XDG_RUNTIME_DIR/obsidian-livesync-init.XXXXXX")
        # 秘密を書いた設定の置き場を、終了時に必ず消す。パスは trap を置く時点で埋め込む: EXIT の trap は
        # init を抜けたあとに走り、local の work はもう無い(set -u で `work: unbound variable` になり、
        # 消えずに残った。2026-11-11 の初回の init で実測)。
        # shellcheck disable=SC2064
        trap "rm -rf -- $(printf '%q' "$work")" EXIT

        # 途中で失敗した init が、古い印を残して daemon を通さないように、先に消す。
        rm -f "$mark"
        mkdir -p "$state" "$db"

        fetch_settings "$work/settings.json"
        "$cli" "$db" --vault "$vault" --settings "$work/settings.json" fetch --force --write-settings

        suffix=$(suffix_of "$work/settings.json") \
          || die "fetch finished but no database suffix was recorded in the settings; not marking as initialised"
        jq -n --arg s "$suffix" '{ additionalSuffixOfDatabaseName: $s }' >"$mark.tmp"
        mv "$mark.tmp" "$mark"
        printf 'obsidian-livesync: initialised (database suffix recorded in %s)\n' "$mark"
      }

      daemon() {
        local work=''${RUNTIME_DIRECTORY:-} suffix
        [ -e "$mark" ] || die "not initialised: run 'obsidian-livesync init' once by hand first"
        [ -d "$vault" ] || die "vault directory does not exist: $vault"
        [ -n "$work" ] || die "daemon runs only under its systemd unit (RUNTIME_DIRECTORY is not set)"

        suffix=$(suffix_of "$mark") || die "the initialised mark has no database suffix: $mark"
        fetch_settings "$work/settings.json"
        # 設定は毎回作り直すので、DB の接尾辞は印のもので上書きする。上書きしないと、
        # 接尾辞の無い設定が空の別 DB を開き、全ファイルの版を作り直す(#1327)。
        jq --arg s "$suffix" '.additionalSuffixOfDatabaseName = $s' "$work/settings.json" >"$work/settings.next.json"
        mv "$work/settings.next.json" "$work/settings.json"

        exec "$cli" "$db" --vault "$vault" --settings "$work/settings.json" daemon
      }

      case "$1" in
        init) init ;;
        daemon) daemon ;;
        *) usage ;;
      esac
    '';
  };
in
{
  options.dotfiles.obsidian.livesync = {
    enable = lib.mkEnableOption ''
      the Self-hosted LiveSync CLI daemon (a per-user service) in place of the plugin,
      and stop autostarting Obsidian. Needs a one-time `obsidian-livesync init` by hand
      (docs/agents/obsidian-livesync-cli.md)
    '';
    secretKey = lib.mkOption {
      type = lib.types.nullOr (lib.types.strMatching "[A-Za-z_][A-Za-z0-9_]*");
      default = null;
      description = ''
        Secrets Manager key (an environment-variable-shaped name) whose value is the
        LiveSync settings JSON. The real name is a machine-state value and stays in the
        private wrapper (ADR-0034). While null, `obsidian-livesync init` and `daemon`
        fail with an error.
      '';
    };
  };

  config = lib.mkIf (cfg.enable && vaultPath != null) {
    assertions = [
      {
        assertion = pkgs.stdenv.isLinux;
        message = "dotfiles.obsidian.livesync is a systemd user service; enable it on Linux only";
      }
    ];

    # PATH に出すのはラッパーだけ。生の CLI は出さない。
    home.packages = [ wrapper ];

    # ログインで起動する常駐の service の pipeline-map のノード(ADR-872)。
    dotfiles.pipeline.timerNodes.obsidian-livesync = "obsidian-livesync";

    systemd.user.services.obsidian-livesync = lib.mkIf pkgs.stdenv.isLinux {
      Unit = {
        Description = "Self-hosted LiveSync CLI daemon for the personal Obsidian vault";
        # Condition でなく Assert にする。初期化の前や vault の不在で起動できないことを、
        # `hms` の degraded と `systemctl --user --failed` に出す(黙って起動しないままにしない)。
        AssertPathExists = mark;
        AssertPathIsDirectory = toString vaultPath;
      };
      Service = {
        # 上流の `deploy/livesync-cli.service`(1.0.44-cli)の値。user の unit には
        # `network-online.target` が無いので After・Wants は置かず、Restart で待つ。
        Type = "exec";
        ExecStart = "${wrapper}/bin/obsidian-livesync daemon";
        Restart = "on-failure";
        RestartSec = 10;
        LimitNOFILE = 65536;
        # 設定(接続情報を含む)は tmpfs の RuntimeDirectory にだけ作り、停止で消える。
        RuntimeDirectory = "obsidian-livesync";
        RuntimeDirectoryMode = "0700";
        UMask = "0077";
        NoNewPrivileges = true;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
