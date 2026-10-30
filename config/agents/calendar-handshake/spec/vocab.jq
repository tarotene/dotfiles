# vocabulary.json から、仕様が import する検査用モジュール vocab.qnt を作る(コミットしない生成物)。
#   jq -r -f vocab.jq ../vocabulary.json > vocab.qnt
# 事象は from × to の組に展開する。TRANSITIONS_LIST の添字が、仕様の被覆の検査(cov<i>)の添字になる。
# 組の by は、by_detail があればそれ(workplace-script など。仕様の参加者の名前)、無ければ by。
# from が ["none"] だけの事象(予定を作る propose)は、仕様の遷移の表に含めない: 仕様の personCreate は
# 遷移の記録(fired)を更新せず、予定の作成は任意の題名・日で起きるため、被覆の検査の対象にできない
# (ADR-1293。仕様と実装の照合は、ここに事象を足す前に別の段で決める)。
def q: "\"" + . + "\"";
[.events[] | select(.from != ["none"]) | . as $e | $e.from[] as $f | $e.to[] as $o
  | { from: $f, to: $o, by: ($e.by_detail // $e.by) }] as $ts
| "module vocab {\n  type Tr = { from: str, to: str, by: str }\n"
  + "  pure val TRANSITIONS_LIST: List[Tr] = [\n"
  + ([$ts[] | "    { from: \(.from | q), to: \(.to | q), by: \(.by | q) }"] | join(",\n"))
  + "\n  ]\n"
  + "  pure val TRANSITIONS: Set[Tr] = TRANSITIONS_LIST.indices().map(i => TRANSITIONS_LIST.nth(i))\n"
  + "  pure val STATES: Set[str] = Set(" + ([.states[].id | q] | join(", ")) + ")\n}\n"
