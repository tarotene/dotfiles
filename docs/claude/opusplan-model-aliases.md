# opusplan-model-aliases — Plan 中だけ別のモデルを使う

`~/.claude/settings.json` の `model: "opusplan"` は「Plan 中は Opus、実行中は
Sonnet」と説明されるが、実際に固定されているのは**モデルではなくエイリアス**で
ある。Plan 側が `opus` エイリアス、実行側が `sonnet` エイリアスを解決するだけで、
各エイリアスがどの具体モデルに解決されるかは別に宣言できる。

この一段の間接参照を使って、`opus` エイリアスだけを Fable 5 に差し替えている
（`home/modules/claude.nix` の `claudeModelEnv`）。結果は
**Plan 中は Fable 5(1M context)、実行中は Sonnet 5**。

配備は `syncModelConfig` / `claudeModelEnv` / `claudeFallbackModels` /
`home.activation.registerClaudeModelConfig`。`registerClaudeHooks` や
`registerClaudePermissions` と同じ「activation 時の冪等 jq マージ」パターンで、
それぞれ独立した activation script として `writeBoundary` の後に並ぶ。

## なぜ env なのか

エイリアスの解決は `ANTHROPIC_DEFAULT_<FAMILY>_MODEL` を**最優先**で読む。
claude 2.1.252 の実装（バイナリから抽出）:

```js
function Hl(){let e=a.ANTHROPIC_DEFAULT_OPUS_MODEL;if(e!==void 0)return Yb(e);return Nt()}
function Nt(e=pc()){return zs("opus",e)??e.opus5}          // カタログ既定
function dp(){let e=a.ANTHROPIC_DEFAULT_SONNET_MODEL;if(e!==void 0)return Yb(e);return xs()}
```

`opusplan` はこの 2 つを permission mode で振り分ける:

```js
function zde(e){if(e==="opusplan"||e==="opusplan[1m]")return"opus";…}
// Plan 側:  let o = t==="opus" ? (r?qe(Hl()):Hl()) : dp();
// 実行側:  case"opusplan": return o?Yb(qe(dp())):dp();
```

つまり `ANTHROPIC_DEFAULT_OPUS_MODEL` を差し替えれば、`opusplan` の Plan 側だけが
追随する。`sonnet` エイリアスは宣言しない — カタログ既定がそのまま望みの値(Sonnet
5)であり、具体 ID を書くとモデル世代が上がったときに古い ID へ固定してしまう。

env の置き場は settings.json の `env` キー（スキーマ上の説明は "Environment
variables to set for Claude Code sessions"）。`home.sessionVariables` ではなく
こちらを選んだのは、(1) 効くのが Claude Code だけでスコープが正確、(2) 次回
ログインを待たず次の `claude` 起動から効く、(3) settings.json への冪等マージが
すでに 3 本ある確立したパターンだから。

## 実測

`claude -p --output-format json` の `modelUsage` で確認した（2.1.252）:

| 設定 | `--model` | permission mode | 解決されたモデル |
|------|-----------|-----------------|------------------|
| env なし | `opus` | — | `claude-opus-5` |
| `ANTHROPIC_DEFAULT_OPUS_MODEL=claude-fable-5` | `opus` | — | `claude-fable-5` |
| 同上 | `opusplan` | `plan` | `claude-fable-5[1m]` |
| 同上 | `opusplan` | `acceptEdits` | `claude-sonnet-5` |
| 同上を settings.json の `env` 経由で | `opusplan` | `plan` | `claude-fable-5[1m]` |

Plan 側に `[1m]` が付くのは Opus Plan Mode 自身の挙動（`e==="opusplan[1m]"||Xb()`
の分岐）で、こちらで指定したものではない。Plan 中は 1M context で読める。

## 副作用: `opus` エイリアスは全体が Fable になる

差し替えの対象はエイリアスなので、`/model opus` を選んでも実体は Fable 5 になる。
`opusplan` の Plan 側だけを狙い撃ちする方法は存在しない（Plan 側の解決経路が
`opus` エイリアスそのものだから）。素の Opus を使いたいときはエイリアスを避けて
モデル名を直接指定する: `/model claude-opus-5`。

## `fallbackModel`

`claudeFallbackModels` は settings スキーマの `fallbackModel`（"Fallback model(s)
tried in order when the primary model is overloaded or unavailable"）に対応する。
Plan 中の Fable が詰まったら Opus 5 に落ちる。

ここは**エイリアスではなく具体 ID を書く**こと。`"opus"` と書くと上の env で
Fable 5 に解決され、fallback が自分自身を指してしまう。

なお `fallback_3p` を使う似た経路（`if(n.family==="fable")return
a.ANTHROPIC_DEFAULT_OPUS_MODEL??…`）はバイナリ内に存在するが、これは
`if(ra())return` で first party では早期 return する Bedrock/Vertex 用の
仕組みであり、この構成には効かない。first party で Fable が使えないときの降格は
Plan 側解決の `clamp:"stepDown"`（org の `availableModels` 制限などで
`opus` へ落とす）と、この `fallbackModel` が担う。

## `model` キーは触らない

`syncModelConfig` は `.env` の宣言キーと `.fallbackModel` だけを書き、`.model` に
一切触らない。`model` は `/model` で日常的に切り替える対象であり、宣言的に固定
すると UI での選択を毎 `switch` で奪ってしまう。逆に言えば
**`model: "opusplan"` 自体は本人が `/model` で選んだ状態に依存する** —
この構成が効くのは `model` が `opusplan` のときだけ。

## 撤回: `retiredModelEnvKeys`

`.env` から宣言を外すときは、キー名を `retiredModelEnvKeys` に移す。activation が
`.env` から該当キーだけを削除し、`.env` が空になればキーごと畳む（「入れる前の形に
戻す」）。無条件 `del` にしないのは、他の経路で入れた env を奪わないため —
`retiredPermissionRules` / `retiredStatusLineCommands` と同型で、
`docs/claude/claude-permissions.md` の「forward switch にしか効かない」注意も
等しく当てはまる。

## 前提のバージョン依存性

上記の関数名（`Hl` / `dp` / `zde`）は minify 後の名前で、バージョンが上がれば
変わる。依存しているのは名前ではなく「エイリアス解決が
`ANTHROPIC_DEFAULT_<FAMILY>_MODEL` を最優先で読む」という公開された契約
（env var 名とスキーマ）だけなので、実装の内部名が変わっても設定は生き続ける。
挙動が怪しくなったら上の実測表を `claude -p --output-format json` で取り直すのが
最短の確認手順。
