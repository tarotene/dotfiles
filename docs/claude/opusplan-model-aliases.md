# opusplan-model-aliases — Plan 中だけ別のモデルを使い、1 コマンドで戻す

`~/.claude/settings.json` の `model: "opusplan"` は「Plan 中は Opus、実行中は
Sonnet」と説明されるが、実際に固定されているのは**モデルではなくエイリアス**で
ある。Plan 側が `opus` エイリアス、実行側が `sonnet` エイリアスを解決するだけで、
各エイリアスがどの具体モデルに解決されるかは別に宣言できる。

この一段の間接参照を使って `opus` エイリアスを Fable に差し替え、**Plan 中は
Fable(1M context)、実行中は Sonnet** を得ている。

差し替えると副作用として `/model opus` も Fable になる。つまり Fable 固有の
リミットが枯れたとき、セッション内から Opus へ戻る道が塞がる。その往復路が
`scripts/claude-plan-model`(`~/.local/bin/claude-plan-model`)である。

```
claude-plan-model          # トグル (fable <-> opus)
claude-plan-model fable    # 明示指定(冪等)
claude-plan-model opus
claude-plan-model status   # 現在のモードを見るだけ
claude-plan-model sync     # モードは保ったまま具体 ID を引き直す(activation 用)
```

## 状態の所有者を 2 つに割っている

| 何 | 誰が持つか | どこに出るか |
|----|-----------|-------------|
| **モード**(fable / opus) | `claude-plan-model`(実行時状態) | `.env.ANTHROPIC_DEFAULT_OPUS_MODEL` の値の系統 |
| **そのモードの具体モデル ID** | 宣言(activation の `sync`) | 同じキーの値そのもの / `.fallbackModel` |

モードを宣言で固定しないのは、`.model` を宣言で固定しないのと同じ理由 —
Fable のリミットが枯れたのを見て本人が倒す一時スイッチであり、`hms` を打った
瞬間に枯れたモデルへ黙って戻されては困る。

逆に具体 ID を Nix に書かないのは、**書くと必ず腐るから**。実際、宣言が
`claude-fable-5` を pin していた一方で、claude 2.1.263 の baked catalog は
`latest_per_family.fable = "claude-fable-5-1"` を指しており、`.model` も
`"claude-fable-5-1[1m]"` になっていた。宣言だけが 1 世代取り残されていた。

## モードの表現

| モード | `.env.ANTHROPIC_DEFAULT_OPUS_MODEL` | `.fallbackModel` |
|--------|--------------------------------------|------------------|
| Fable | `<latest_per_family.fable>` | `[<latest_per_family.opus>]` |
| Opus | `<latest_per_family.opus>` | 削除 |
| 未初期化 | キー不在 | — |

`sync` の判定はキーの値の系統だけを見る: 不在 → Fable で seed、`claude-fable-*`
→ Fable 継続、`claude-opus-*` → Opus 継続、それ以外 → **据え置き + 警告**
(他の経路で入れた値を奪わない)。

Opus モードを「キー削除」ではなく**具体 Opus ID の書き込み**で表すのが要点。
キー不在を「未初期化」の意味に温存しないと、`sync` が新規マシンと意図的な Opus
モードを区別できず、`hms` のたびに Fable へ引き戻してしまう。

`fallbackModel` を Opus モードで消すのは、Claude Code が「fallback が main
model と同じ」を拒否するため("Fallback model cannot be the same as the main
model.")。Sonnet に落とす選択肢もあるが、既に縮退した Plan からさらに落ちると
Plan の質が読めなくなるので採らない。

## なぜ env なのか

エイリアスの解決は `ANTHROPIC_DEFAULT_<FAMILY>_MODEL` を**最優先**で読む。
claude 2.1.263 の実装(バイナリから抽出、名前は minify 後のもの):

```js
function $l(){let e=a.ANTHROPIC_DEFAULT_OPUS_MODEL;if(e!==void 0)return cS(e);return rl("opus")??en()}
function en(e=ec()){return nl("opus",e)??e.opus5}          // カタログ既定
function Mp(){let e=a.ANTHROPIC_DEFAULT_SONNET_MODEL;if(e!==void 0)return cS(e);return rl("sonnet")??Za()}
```

`opusplan` はこの 2 つを permission mode で振り分ける(Plan 側が `$l()`、実行側が
`Mp()`)。つまり `ANTHROPIC_DEFAULT_OPUS_MODEL` を差し替えれば、`opusplan` の
Plan 側だけが追随する。`sonnet` エイリアスは宣言しない — カタログ既定がそのまま
望みの値であり、具体 ID を書くとモデル世代が上がったときに古い ID へ固定して
しまう。

env の置き場は settings.json の `env` キー。`home.sessionVariables` ではなく
こちらを選んだのは、(1) 効くのが Claude Code だけでスコープが正確、(2) 次回
ログインを待たず次の `claude` 起動から効く、(3) settings.json への冪等マージが
すでに確立したパターンだから。

## エイリアス文字列は env の値として使えない

`ANTHROPIC_DEFAULT_OPUS_MODEL=fable` と書けば世代追従できそうに見えるが、**できない**。
値は恒等関数(`function cS(e){return e}`)を通ってそのまま渡るだけで、API が拒否する:

```
$ claude --settings /tmp/s.json -p --model opus --output-format json 'say ok'
[claude-code:unrecognized_model] {"model":"fable","query_source":"sdk"}
```

`modelOverrides` も使えない。これは Anthropic ID → provider 固有 ID(Bedrock の
inference profile ARN 等)の写像であって、permission mode 別のモデル指定ではない。

**したがって opus エイリアスを乗っ取る以上、具体 ID は構造的に必須**であり、
その具体 ID を腐らせない仕組みが `latest_per_family` の引き直しである。

## `latest_per_family` の引き直し

インストール済みの claude バイナリに、モデルカタログの最新表が焼かれている:

```
$ LC_ALL=C grep -aom1 'latest_per_family:{[^}]\+}' "$(readlink -f "$(command -v claude)")"
latest_per_family:{fable:"claude-fable-5-1",opus:"claude-opus-5",sonnet:"claude-sonnet-5",haiku:"claude-haiku-4-5"}
```

- `strings` ではなく `grep -a` を直に使うので binutils に依存しない。`-m1` で
  早期打ち切りするため 215MB のバイナリでも ~50ms。
- 空の `latest_per_family:{}` も別の場所に現れるので、パターンは中身が 1 文字
  以上あることを要求する(`[^}]\+`)。
- 抽出に失敗したとき・`claude` が未インストールのときは **settings.json に
  一切触らず警告のみ**。起動する claude が無いのに env だけ置いても意味が無く、
  中途半端な model 設定のほうが有害だから。

追従の粒度は「**インストール済み CLI のバージョン**」になる。サーバ側 catalog が
先行しても、CLI が自動更新されるまでは気付かない。引き直しの契機は `hms`
(activation)と、トグル実行時の 2 つ。SessionStart フックにはしていない — 毎起動で
バイナリを読むコストを払ってまで得るものが「1 セッション遅れて効く自己修復」しか
ないため。

## 実測の落とし穴: settings.json の env は shell env を上書きする

`FOO=bar claude ...` 形式で挙動を確かめると**嘘の結果が出る**。settings.json の
`env` は起動時に process.env へ書き込まれ、**launch 時の shell env を上書きする**
(`if(RUe(e,o))process.env[e]=o`)。settings.json に既に
`ANTHROPIC_DEFAULT_OPUS_MODEL` があると、シェルで何を渡しても settings.json の値が
勝つ。

**検証は必ず `--settings <tmpfile>` で行うこと。**

```bash
printf '{"env":{"ANTHROPIC_DEFAULT_OPUS_MODEL":"claude-opus-5"}}' > /tmp/s.json
claude --settings /tmp/s.json --permission-mode plan -p --model opusplan \
  --output-format json 'say ok' | jq '.modelUsage | keys'
```

同じ理由で、**env は起動時に焼き込まれるので切り替えは実行中セッションに効かない**。
`claude-plan-model` は毎回その旨を出力する。

## 実測 (claude 2.1.263)

上の手順(`--settings` 経由)で取得した `modelUsage` のキー:

| `.env.ANTHROPIC_DEFAULT_OPUS_MODEL` | `--model` | permission mode | 解決されたモデル |
|-------------------------------------|-----------|-----------------|------------------|
| `claude-fable-5-1` | `opusplan` | `plan` | `claude-fable-5-1[1m]` |
| `claude-fable-5-1` | `opusplan` | `acceptEdits` | `claude-sonnet-5` |
| `claude-opus-5` | `opusplan` | `plan` | `claude-opus-5[1m]` |
| `fable`(エイリアス文字列) | `opus` | — | **`unrecognized_model` エラー** |

Plan 側に `[1m]` が付くのは Opus Plan Mode 自身の挙動で、こちらで指定したもので
はない。Plan 中は 1M context で読める。

## リミットが枯れたときの手順

1. `claude-plan-model`(トグル) — 次に起動する claude から Opus モードになる
2. 実行中のセッションは、次のどちらか
   - `claude --continue` で再起動する(**推奨**。opusplan の Plan/実行の分離を保てる)
   - `/model claude-opus-5` と直に指定する(再起動不要だが、そのセッションだけ
     Plan/実行の分離が消えてフラットな Opus になる)
3. Fable のリミット窓がリセットしたら `claude-plan-model` をもう一度打つ

herdr で並列にエージェントを回している場合、**既に起動済みのペインはすべて元の
モードのまま**である点に注意。

## `fallbackModel` はリミット枯渇を救わない

これがこのトグルの存在理由である。claude 2.1.263 で fallback が発火するのは:

- `model_not_found` / `permission_denied` / `server_error`
- overload(429 / 529)

の 4 経路だけで、**「Usage limit reached」は別経路**(リセットまで待つループ、
`/rate-limit-options` で制御する)に流れる。Fable の独立したリミットが枯れても
fallback は動かない。

したがって `fallbackModel` が守っているのは「一過性の overload」であり、
「リミット枯渇」は人間が `claude-plan-model` で倒す。役割は重複しない。

なお `fallback_3p` を使う似た経路(`if(n.family==="fable")return
a.ANTHROPIC_DEFAULT_OPUS_MODEL??…`)はバイナリ内に存在するが、これは first party
では早期 return する Bedrock/Vertex 用の仕組みであり、この構成には効かない。

## 前提: `.model` が `"opusplan"` であること

この構成が効くのは `.model == "opusplan"`(または `"opusplan[1m]"`)のときだけ。
フラットなモデル ID を選んでいると Plan/実行の分離自体が存在せず、エイリアスの
差し替えは無意味になる。

`.model` は `/model` で本人が日常的に切り替える対象なので、home-manager も
`claude-plan-model` も**書き換えない**。代わりに `claude-plan-model` が
`opusplan` でないことを検出したら警告する(`sync` では黙る — activation は
本人の操作ではないため)。

## 副作用: `opus` エイリアスは全体が Fable になる

差し替えの対象はエイリアスなので、Fable モード中は `/model opus` を選んでも実体は
Fable になる。`opusplan` の Plan 側だけを狙い撃ちする方法は存在しない(Plan 側の
解決経路が `opus` エイリアスそのものだから)。素の Opus を一時的に使いたいときは
エイリアスを避けてモデル名を直接指定する: `/model claude-opus-5`。

## 前提のバージョン依存性

上の関数名(`$l` / `Mp` / `en`)は minify 後の名前で、バージョンが上がれば変わる。
依存しているのは名前ではなく「エイリアス解決が `ANTHROPIC_DEFAULT_<FAMILY>_MODEL`
を最優先で読む」という公開された契約(env var 名とスキーマ)だけなので、実装の
内部名が変わっても設定は生き続ける。

一方 `latest_per_family` の抽出は minify されたリテラルに依存する。壊れても
settings.json は据え置かれ、警告だけが出る(pin が固まるだけで、設定が消えたり
壊れたりはしない)。挙動が怪しくなったら、上の `--settings` を使った手順で
実測を取り直すのが最短の確認手順。
