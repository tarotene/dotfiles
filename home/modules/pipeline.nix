# pipeline-map(ADR-872、pipeline-map.toml)の、nix 側の登録のうち、systemd timer と
# launchd agent を、評価時に確かめる。
#
# 各モジュールは、timer / agent を定義するそばに
#
#   dotfiles.pipeline.timerNodes.<名前> = "<pipeline-map のノード id>";
#
# を書く。<名前> は `systemd.user.timers.<名前>` / `launchd.agents.<名前>` の名前。同じ名前の
# systemd timer と launchd agent は、1 行で足りる。ここは最終の config(全モジュールの合成後)の
# timer と agent の名前を列挙して、対応付けの無いものと、manifest に無い id を `assertions` で落とす。
# 生の `systemd.user.timers.*` を書いても、最終の config に現れるので、迂回できない。
#
# 逆向き(auto = true のノードに登録が 1 つも無い)は、ここでは確かめない。`lib.mkIf (isLinux && …)` の
# 条件付きの登録があり、1 ホストの評価では、ほかのホストの登録を見られないため。`cargo test`
# (crates/pipeline-map)が、対応付けの行を静的に走査して確かめる。
#
# activation・registrar・statusline は、nix の式として評価できる形でなく(シェル文字列の引数)、
# 今までどおり直前のコメント `# pipeline-node: <id>` で、`cargo test` が検査する。
{ config, lib, ... }:
let
  cfg = config.dotfiles.pipeline;

  pipelineMap = builtins.fromTOML (builtins.readFile ../../pipeline-map.toml);

  # 上流のモジュール(home-manager の `services.*` など)が作る timer / agent。こちらの
  # 機構ではないので、ノードを持たない。足すのは、上流が作ると確かめたものだけ。
  upstreamNames = [
    # `services.gpg-agent`(gpg.nix)が作る launchd agent(home-manager の
    # modules/services/gpg-agent.nix、`launchd.agents.gpg-agent`)。ソケット起動のデーモンで、
    # pipeline-map の対象(自動発火する機構)の外。
    "gpg-agent"
  ];

  registeredNames = lib.unique (
    builtins.attrNames config.systemd.user.timers ++ builtins.attrNames config.launchd.agents
  );
  unmapped = lib.filter (
    n: !(cfg.timerNodes ? ${n}) && !(builtins.elem n upstreamNames)
  ) registeredNames;
  undefinedIds = lib.filterAttrs (_: id: !(builtins.elem id cfg.nodeIds)) cfg.timerNodes;
in
{
  options.dotfiles.pipeline = {
    nodeIds = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      readOnly = true;
      internal = true;
      default = map (n: n.id) pipelineMap.node;
      description = ''
        pipeline-map.toml に定義されたノードの id。Claude hook の宣言の `node`
        (agents.nix)と、`timerNodes` の値が、ここに無いと評価が落ちる。
      '';
    };

    timerNodes = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = {
        due-remind = "due-remind";
      };
      description = ''
        `systemd.user.timers.<名前>` / `launchd.agents.<名前>` の名前から、pipeline-map の
        ノード id への対応。timer と agent を足したのに対応付けが無い、または id が
        pipeline-map.toml に無いと、評価が落ちる。
      '';
    };
  };

  config.assertions = [
    {
      assertion = unmapped == [ ];
      message = ''
        dotfiles.pipeline.timerNodes: 次の systemd timer / launchd agent に、pipeline-map のノードの対応付けが無い: ${lib.concatStringsSep ", " unmapped}
        その定義のそばに `dotfiles.pipeline.timerNodes.<名前> = "<ノード id>";` を足す(ノードは pipeline-map.toml、ADR-872)。
        上流のモジュールが作るものなら、home/modules/pipeline.nix の upstreamNames に足す。'';
    }
    {
      assertion = undefinedIds == { };
      message = "dotfiles.pipeline.timerNodes: pipeline-map.toml に定義の無いノード id: ${
        lib.concatStringsSep ", " (lib.mapAttrsToList (n: id: "${n} = ${id}") undefinedIds)
      }";
    }
  ];
}
