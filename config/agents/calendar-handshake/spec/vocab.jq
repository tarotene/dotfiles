# vocabulary.json から、仕様が import する検査用モジュール vocab.qnt を作る(コミットしない生成物)。
#   jq -r -f vocab.jq ../vocabulary.json > vocab.qnt
# 遷移は from × to の組に展開する。TRANSITIONS_LIST の添字が、仕様の被覆の検査(cov<i>)の添字になる。
def q: "\"" + . + "\"";
[.transitions[] | . as $t | $t.from[] as $f | $t.to[] as $o | { from: $f, to: $o, by: $t.by }] as $ts
| "module vocab {\n  type Tr = { from: str, to: str, by: str }\n"
  + "  pure val TRANSITIONS_LIST: List[Tr] = [\n"
  + ([$ts[] | "    { from: \(.from | q), to: \(.to | q), by: \(.by | q) }"] | join(",\n"))
  + "\n  ]\n"
  + "  pure val TRANSITIONS: Set[Tr] = TRANSITIONS_LIST.indices().map(i => TRANSITIONS_LIST.nth(i))\n"
  + "  pure val STATES: Set[str] = Set(" + ([.states[].id | q] | join(", ")) + ")\n}\n"
