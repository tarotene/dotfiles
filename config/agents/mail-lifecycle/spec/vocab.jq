# vocabulary.json から、仕様が import する検査用モジュール vocab.qnt を作る(コミットしない生成物)。
#   jq -r -f vocab.jq ../vocabulary.json > vocab.qnt
# 出来事は from と to の要素ごとに (ev, from, to, by) の組へ展開する。to が ["same"] の出来事(remind)は、
# 組の to を from と同じにする(状態は変わらない)。TRANSITIONS_LIST の添字が、仕様の被覆の検査
# (cov<i>)の添字になる。OPENING は from が ["none"] だけの出来事(読む・下書き・質問)、
# TERM_EVENTS は終端の状態へ入りうる出来事。
# 語彙の共通の形(config/agents/vocabulary/vocabulary.schema.json)では to は常に配列。
def q: "\"" + . + "\"";
def qs: [.[] | q] | join(", ");
([.states[] | select(.terminal) | .id]) as $term
| ([.states[] | select(.terminal | not) | .id]) as $unres
| [.events[] | . as $e | $e.from[] as $f | $e.to[] as $t
   | { ev: $e.id, from: $f, to: (if $t == "same" then $f else $t end), by: $e.by }] as $ts
| ([.events[] | select(.from == ["none"]) | .id]) as $open
| ([$ts[] | select(.to as $t | $term | any(. == $t)) | .ev] | unique) as $termev
| "module vocab {\n  type Tr = { ev: str, from: str, to: str, by: str }\n"
  + "  pure val TRANSITIONS_LIST: List[Tr] = [\n"
  + ([$ts[] | "    { ev: \(.ev | q), from: \(.from | q), to: \(.to | q), by: \(.by | q) }"] | join(",\n"))
  + "\n  ]\n"
  + "  pure val TRANSITIONS: Set[Tr] = TRANSITIONS_LIST.indices().map(i => TRANSITIONS_LIST.nth(i))\n"
  + "  pure val INITIAL: str = \(.initial | q)\n"
  + "  pure val STATES: Set[str] = Set(" + ([.states[].id] | qs) + ")\n"
  + "  pure val TERMINAL: Set[str] = Set(" + ($term | qs) + ")\n"
  + "  pure val UNRESOLVED: Set[str] = Set(" + ($unres | qs) + ")\n"
  + "  pure val EVENTS: Set[str] = Set(" + ([.events[].id] | qs) + ")\n"
  + "  pure val OPENING: Set[str] = Set(" + ($open | qs) + ")\n"
  + "  pure val TERM_EVENTS: Set[str] = Set(" + ($termev | qs) + ")\n}\n"
