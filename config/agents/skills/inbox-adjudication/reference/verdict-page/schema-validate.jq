# schema-validate.jq — JSON Schema(draft 2020-12)の部分集合の検証器。build.sh と、
# scripts/vocab-graph-check(語彙 JSON の Schema 検査、ADR-0000)が使う。
#
# 呼び出し: jq -nr --slurpfile s <schema.json> --slurpfile d <instance.json> -f schema-validate.jq
# 出力: 違反 1 件につき 1 行。0 行なら適合。
#
# 対応するキーワードは、このディレクトリの 2 つのスキーマと、語彙 JSON のスキーマ
# (config/agents/vocabulary/vocabulary.schema.json)が使う分だけ(下の known)。
# 対応していないキーワード(例 uniqueItems、patternProperties)がスキーマに現れたら、黙って
# 無視せず違反として出す。検証器が理解しないスキーマを「通る」ことにしないため。
# format は 2020-12 の仕様で注釈(検証しない既定)なので、検証せず許す。
# $ref は同じファイルの中(#/$defs/...)だけ。

def annot: ["$schema", "$id", "$comment", "$defs", "title", "description", "default", "examples", "format"];
def known: ["type", "const", "enum", "required", "properties", "additionalProperties", "items",
            "minItems", "maxItems", "minLength", "maxLength", "pattern", "$ref", "oneOf", "anyOf"];

def jtype: if type == "number" then (if . == floor then "integer" else "number" end) else type end;
def type_ok($t): jtype as $j | $t == $j or ($t == "number" and $j == "integer");

def refto($root):
  if startswith("#/") then . as $r | $root | getpath($r | split("/")[1:])
  else error("未対応の $ref: \(.)") end;

def check($root; $s; $p):
  . as $x
  | ($x | jtype) as $jt
  | ($s | keys_unsorted - (annot + known)) as $unk
  | ( if ($unk | length) > 0 then ["\($p): 未対応のキーワード \($unk | join(","))"] else [] end )
  + ( if $s | has("$ref") then ($x | check($root; ($s["$ref"] | refto($root)); $p)) else [] end )
  + ( if $s | has("type") then
        ([$s.type] | flatten) as $ts
        | if any($ts[]; . as $t | $x | type_ok($t)) then []
          else ["\($p): 型が \($ts | join("|")) でない(\($jt))"] end
      else [] end )
  + ( if $s | has("const") then
        (if $x == $s.const then [] else ["\($p): \($s.const | tojson) でない(\($x | tojson))"] end)
      else [] end )
  + ( if $s | has("enum") then
        (if any($s.enum[]; . == $x) then [] else ["\($p): \($s.enum | tojson) のどれでもない(\($x | tojson))"] end)
      else [] end )
  + ( if ($s | has("required")) and $jt == "object" then
        [ $s.required[] | select(. as $k | $x | has($k) | not) | "\($p): 必須の \(.) が無い" ]
      else [] end )
  + ( if ($s | has("properties")) and $jt == "object" then
        [ $s.properties | to_entries[] | select(. as $e | $x | has($e.key)) | . as $e
          | ($x[$e.key] | check($root; $e.value; "\($p).\($e.key)"))[] ]
      else [] end )
  + ( if ($s | has("additionalProperties")) and $jt == "object" then
        $s.additionalProperties as $ap
        | if $ap == false then
            [ $x | keys_unsorted[] | select(. as $k | ($s.properties // {}) | has($k) | not) | "\($p): 未定義の \(.) がある" ]
          elif $ap == true then []
          elif ($ap | type) == "object" then
            # properties に無いキーの値は、このスキーマに従う(語彙 JSON の timers が使う)
            [ $x | to_entries[] | select(. as $e | ($s.properties // {}) | has($e.key) | not) | . as $e
              | ($e.value | check($root; $ap; "\($p).\($e.key)"))[] ]
          else ["\($p): additionalProperties は true・false・スキーマだけに対応"] end
      else [] end )
  + ( if ($s | has("items")) and $jt == "array" then
        [ range(0; $x | length) as $i | ($x[$i] | check($root; $s.items; "\($p)[\($i)]"))[] ]
      else [] end )
  + ( if $jt == "array" and ($s | has("minItems")) and ($x | length) < $s.minItems
      then ["\($p): 要素が \($s.minItems) 個未満"] else [] end )
  + ( if $jt == "array" and ($s | has("maxItems")) and ($x | length) > $s.maxItems
      then ["\($p): 要素が \($s.maxItems) 個を超える"] else [] end )
  + ( if $jt == "string" and ($s | has("minLength")) and ($x | length) < $s.minLength
      then ["\($p): \($s.minLength) 文字未満"] else [] end )
  + ( if $jt == "string" and ($s | has("maxLength")) and ($x | length) > $s.maxLength
      then ["\($p): \($s.maxLength) 文字を超える"] else [] end )
  + ( if $jt == "string" and ($s | has("pattern")) and ($x | test($s.pattern) | not)
      then ["\($p): パターン \($s.pattern) に合わない(\($x | tojson))"] else [] end )
  + ( if $s | has("oneOf") then
        ([ $s.oneOf[] | . as $sub | $x | check($root; $sub; $p) | select(length == 0) ] | length) as $n
        | if $n == 1 then [] else ["\($p): oneOf に一致するのが \($n) 件(1 件だけでなければならない)"] end
      else [] end )
  + ( if $s | has("anyOf") then
        ([ $s.anyOf[] | . as $sub | $x | check($root; $sub; $p) | select(length == 0) ] | length) as $n
        | if $n >= 1 then [] else ["\($p): anyOf のどれにも一致しない"] end
      else [] end );

$d[0] | check($s[0]; $s[0]; "$") | .[]
