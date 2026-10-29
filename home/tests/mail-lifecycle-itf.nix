# 未裁定のメール項目の寿命の仕様(config/agents/mail-lifecycle/spec/lifecycle.qnt)の軌跡。
#
# `quint run` の乱数の探索で、環境の手(受信・同期の遅れ・落ちる・行の削除・日の進み)を全部入れた
# 軌跡(ITF)を数十本作る。種を固定するので、同じ入力から同じ軌跡ができる。
# crates/worklog/tests/itf_replay.rs が、各状態のノートの行の写し(views)を実装の畳み込みに通し、
# 仕様の畳み込みの結果(fo)と一致することを確かめる。置き場は環境変数 MAIL_LIFECYCLE_ITF_DIR で渡す。
# CI は .github/workflows/ci.yml の mail-lifecycle-spec ジョブが build して、cargo test に渡す。
{ pkgs, lib }:
let
  root = ../../config/agents/mail-lifecycle;
  spec = root + "/spec";
  table = builtins.fromJSON (builtins.readFile (spec + "/scenarios.json"));

  # 環境の手を全部入れる。4 つの項目、2 つの機体、台帳つき。
  flags = table.defaults // {
    N_KEYS = 4;
    TWO_MACHINES = true;
    WITH_LEDGER = true;
    WITH_DUE = true;
    CAN_DEL_DRAFT = true;
    CAN_SEND = true;
    CAN_DEL_U = true;
    CAN_DEL_T = true;
    CAN_CRASH = true;
    MAX_CRASHES = 2;
    MAX_PERSON = 3;
    SWEEP_ALL = true;
  };
  render = v: if builtins.isBool v then (if v then "true" else "false") else toString v;
  cfg = pkgs.writeText "cfg.qnt" ''
    module cfg {
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (k: v: "  pure val ${k} = ${render v}") flags)}
    }
  '';
in
pkgs.runCommand "mail-lifecycle-itf"
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
    cp ${spec + "/lifecycle.qnt"} lifecycle.qnt
    jq -r -f ${spec + "/vocab.jq"} ${root + "/vocabulary.json"} > vocab.qnt
    cp ${cfg} cfg.qnt
    quint run --main lifecycle --seed 0x5eed --max-samples 60 --max-steps 60 \
      --n-traces 60 --out-itf "$out/trace_{seq}.itf.json" lifecycle.qnt > $out/run.log 2>&1 \
      || { cat $out/run.log; exit 1; }
    n=$(ls $out/*.itf.json | wc -l)
    test "$n" -gt 0 || { cat $out/run.log; echo "軌跡ができなかった"; exit 1; }
  ''
