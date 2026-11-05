# vocabulary.json から、仕様が import する検査用モジュール vocab.qnt を作る(コミットしない生成物)。
#   jq -r -f vocab.jq ../vocabulary.json > vocab.qnt
# 事象は 種類 × from × to の組に展開する。TRANSITIONS_LIST の添字が、仕様の被覆の検査(cov<i>)の添字になる。
# 並びは、語彙の kinds の順(既定の種類が先)→事象の順→from→to。既定の種類の組の並びは、種類を持たなかった
# 版 3 までの並びと同じである(cov0 から cov14 の添字が動かない)。
# 組の by は、by_detail があればそれ(workplace-script など。仕様の参加者の名前)、無ければ by。
# 種類の組は、その種類の事象だけで none から到達できる from に限る。到達できない from の組(remote-trip の
# withdraw の go・blocked・secured からの取り下げ)は、仕様が起こせず、被覆の検査で「死んだ遷移」になるので展開しない。
# from が ["none"] だけの事象(予定を作る propose)は、仕様の遷移の表に含めない: 仕様の personCreate は
# 遷移の記録(fired)を更新せず、予定の作成は任意の題名・日で起きるため、被覆の検査の対象にできない
# (ADR-1293。仕様と実装の照合は、ここに事象を足す前に別の段で決める)。
def q: "\"" + . + "\"";
def step($evs): . as $s
  | ($s + [ $evs[] | select(any(.from[]; IN($s[]))) | .to[] | select(. != "same") ]) | unique;
. as $v
| [ $v.kinds[] | .id as $k
    | [ $v.events[] | select(any(.kinds[]; . == $k)) ] as $kev
    | (["none"] | until(step($kev) == .; step($kev))) as $reach
    | $kev[] | select(.from != ["none"]) | . as $e | $e.from[] as $f | select($f | IN($reach[])) | $e.to[] as $o
    | { from: $f, to: $o, by: ($e.by_detail // $e.by), kind: $k } ] as $ts
| "module vocab {\n  type Tr = { from: str, to: str, by: str, kind: str }\n"
  + "  pure val TRANSITIONS_LIST: List[Tr] = [\n"
  + ([$ts[] | "    { from: \(.from | q), to: \(.to | q), by: \(.by | q), kind: \(.kind | q) }"] | join(",\n"))
  + "\n  ]\n"
  + "  pure val TRANSITIONS: Set[Tr] = TRANSITIONS_LIST.indices().map(i => TRANSITIONS_LIST.nth(i))\n"
  + "  pure val STATES: Set[str] = Set(" + ([.states[].id | q] | join(", ")) + ")\n"
  + "  pure val KINDS: Set[str] = Set(" + ([.kinds[].id | q] | join(", ")) + ")\n}\n"
