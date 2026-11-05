# 握手の仕様(config/agents/calendar-handshake/spec/handshake.qnt)のモデル検査。
#
# scenarios.json の 1 行を 1 つの derivation にして並列に走らせる。Apalache のサーバーは
# 8822 番ポートを使うので、同じ sandbox の中で 2 つ同時には回せない。nix の sandbox は
# derivation ごとに分かれ、ネットワークも使わない(Quint に同梱の Apalache と TLC だけで動く)。
# 語彙の遷移の被覆(per_transition)の行数は、語彙 JSON の遷移の数から数える。
# CI は .github/workflows/ci.yml の calendar-handshake-spec ジョブが build する
# (flake-check は --no-build で、評価しか行わない)。
{ pkgs, lib }:
let
  root = ../../config/agents/calendar-handshake;
  spec = root + "/spec";
  table = builtins.fromJSON (builtins.readFile (spec + "/scenarios.json"));
  vocabulary = builtins.fromJSON (builtins.readFile (root + "/vocabulary.json"));

  # 種類 × from × to に展開した遷移の数。handshake.qnt の cov0..cov23 を超えたら足りない。
  # from が ["none"] だけの事象(予定を作る propose)は、仕様の遷移の表に含めない(spec/vocab.jq と同じ)。
  # 種類の組は、その種類の事象だけで none から到達できる from に限る(spec/vocab.jq と同じ)。
  kindEvents = k: builtins.filter (e: builtins.elem k.id e.kinds) vocabulary.events;
  reachable =
    evs:
    lib.converge (
      s:
      lib.unique (
        s
        ++ lib.concatMap (
          e: lib.optionals (lib.any (f: builtins.elem f s) e.from) (lib.remove "same" e.to)
        ) evs
      )
    ) [ "none" ];
  kindTransitionCount =
    k:
    let
      evs = kindEvents k;
      reach = reachable evs;
    in
    lib.foldl' (
      n: e: n + builtins.length (builtins.filter (f: builtins.elem f reach) e.from) * builtins.length e.to
    ) 0 (builtins.filter (e: e.from != [ "none" ]) evs);
  transitionCount = lib.foldl' (n: k: n + kindTransitionCount k) 0 vocabulary.kinds;

  expand =
    s:
    if s.per_transition or false then
      lib.genList (
        i:
        (builtins.removeAttrs s [ "per_transition" ])
        // {
          name = "${s.name}_${toString i}";
          invariant = [ "cov${toString i}" ];
        }
      ) transitionCount
    else
      [ s ];
  scenarios = lib.concatMap expand table.scenarios;

  render = v: if builtins.isBool v then (if v then "true" else "false") else toString v;
  cfgFor =
    s:
    let
      flags =
        table.defaults // lib.optionalAttrs ((s.rules or "revised") == "asis") table.asis_rules // s.flags;
    in
    pkgs.writeText "cfg.qnt" ''
      module cfg {
      ${lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "  pure val ${k} = ${render v}") flags)}
      }
    '';

  run =
    s:
    pkgs.runCommand "calendar-handshake-${s.name}"
      {
        nativeBuildInputs = [
          pkgs.quint
          pkgs.jdk
          pkgs.jq
        ];
      }
      ''
        mkdir -p $out work
        cd work
        cp ${spec + "/handshake.qnt"} handshake.qnt
        jq -r -f ${spec + "/vocab.jq"} ${root + "/vocabulary.json"} > vocab.qnt
        cp ${cfgFor s} cfg.qnt
        set +e
        quint verify --backend tlc --main handshake \
          ${lib.optionalString (s ? invariant) "--invariant ${lib.concatStringsSep "," s.invariant}"} \
          ${lib.optionalString (s ? temporal) "--temporal ${lib.concatStringsSep "," s.temporal}"} \
          handshake.qnt > $out/log.txt 2>&1
        set -e
        if [ "${s.expect}" = pass ]; then
          grep -q '\[ok\] No violation found' $out/log.txt || { cat $out/log.txt; echo "期待は pass だが通らなかった: ${s.name}"; exit 1; }
        else
          grep -q 'found a counterexample' $out/log.txt || { cat $out/log.txt; echo "期待は fail(反例が出る)だが出なかった: ${s.name}"; exit 1; }
        fi
      '';
in
assert transitionCount <= 24;
pkgs.linkFarm "calendar-handshake-spec" (
  map (s: {
    name = s.name;
    path = run s;
  }) scenarios
)
