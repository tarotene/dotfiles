# opusplan-model-aliases — Plan/実行のモデルをペアで切り替える

`~/.claude/settings.json` の `model: "opusplan"` は「Plan 中は Opus、実行中は
Sonnet」と説明されるが、実際に固定されているのは**モデルではなくエイリアス**で
ある。Plan 側が `opus` エイリアス、実行側が `sonnet` エイリアスを解決するだけで、
各エイリアスがどの具体モデルに解決されるかは別に宣言できる。

**したがってここでのモードは常に (Plan 側, 実行側) のペアであり、ペアとして
名前を付ける。** 片側だけを指す名前(旧 `fable` / `opus`)は「Plan=Opus,
実行=Fable」のように誤読されるので使わない。

| モード | Plan 側 | 実行側 | 用途 |
|--------|---------|--------|------|
| `fable/sonnet` | Fable(1M) | Sonnet | 既定。Plan を安く長く回す |
| `opus/sonnet` | Opus | Sonnet | Fable のリミットが枯れたときの退避先 |
| `fable/opus` | Fable(1M) | Opus | Plan は Fable のまま、実装を Opus に任せる |

`opus` エイリアスを差し替えると副作用として `/model opus` も Fable になる。つまり
Fable 固有のリミットが枯れたとき、セッション内から Opus へ戻る道が塞がる。その
往復路が `scripts/claude-plan-model`(`~/.local/bin/claude-plan-model`)である。

```
claude-plan-model              # 巡回 (fable/sonnet -> opus/sonnet -> fable/opus -> ...)
claude-plan-model fable/opus   # 明示指定(冪等)。旧名 fable / opus も別名として受ける
claude-plan-model status       # 現在のモードを見るだけ
claude-plan-model sync         # モードは保ったまま具体 ID を引き直す(activation 用)
claude-plan-model --force ...  # .model が "opusplan" でなくても強行する
claude-plan-model --selftest   # 状態機械の self-test(CI で走る)
```

引数なしを 3 モードの巡回にしているのは、「1 コマンドで倒す」という使い方を
保つため。元のモードへ戻るのに 2 回叩くことになるが、方向を覚える必要がない。

## 状態の所有者を 2 つに割っている

| 何 | 誰が持つか | どこに出るか |
|----|-----------|-------------|
| **モード**(ペア 3 種) | `claude-plan-model`(実行時状態) | 2 つの env キーの値の系統 |
| **そのモードの具体モデル ID** | 宣言(activation の `sync`) | 同じキーの値そのもの / `.fallbackModel` |

モードを宣言で固定しないのは、`.model` を宣言で固定しないのと同じ理由 —
Fable のリミットが枯れたのを見て本人が倒す一時スイッチであり、`hms` を打った
瞬間に枯れたモデルへ黙って戻されては困る。

逆に具体 ID を Nix に書かないのは、**書くと必ず腐るから**。実際、宣言が
`claude-fable-5` を pin していた一方で、claude 2.1.263 の baked catalog は
`latest_per_family.fable = "claude-fable-5-1"` を指しており、`.model` も
`"claude-fable-5-1[1m]"` になっていた。宣言だけが 1 世代取り残されていた。

## モードの表現

| モード | `..._OPUS_MODEL`(Plan 側) | `..._SONNET_MODEL`(実行側) | `.fallbackModel` |
|--------|---------------------------|----------------------------|------------------|
| `fable/sonnet` | `<latest_per_family.fable>` | 削除 | `[<latest_per_family.opus>]` |
| `opus/sonnet` | `<latest_per_family.opus>` | 削除 | 削除 |
| `fable/opus` | `<latest_per_family.fable>` | `<latest_per_family.opus>` | `[<latest_per_family.sonnet>]` |
| 未初期化 | キー不在 | (不問) | — |

判定はキーの値の系統だけを見る。Plan 側: 不在 → `fable/sonnet` で seed、
`claude-fable-*` → Fable、`claude-opus-*` → Opus、それ以外 → **据え置き + 警告**。
実行側: 不在 → Sonnet(エイリアス既定がそのまま望みの値)、`claude-opus-*` → Opus、
それ以外 → **据え置き + 警告**。他の経路で入れた値を奪わないという規律なので、
具体 Sonnet ID(こちらが書かない値)が入っていたら Sonnet モードと**みなさない**。

`opus/sonnet` を「キー削除」ではなく**具体 Opus ID の書き込み**で表すのが要点。
キー不在を「未初期化」の意味に温存しないと、`sync` が新規マシンと意図的に選んだ
モードを区別できず、`hms` のたびに既定へ引き戻してしまう。実行側は逆で、Sonnet
実行はエイリアス既定なので**キー不在**で表す — つまり `*/sonnet` の 2 モードの
バイト列は SONNET キーを導入する前と同一で、移行処理が要らない。ただし
`fable/opus` から抜けるときは SONNET キーを**必ず削除する**(残骸が残るとモードが
嘘になる)。

`fallbackModel` はモードごとに置き、**そのモードが既に使っているモデルには
決してしない**(Claude Code が「fallback が main model と同じ」を拒否するため:
"Fallback model cannot be the same as the main model.")。

- `fable/sonnet` → 最新 Opus。縮退した Plan からの脱出口
- `opus/sonnet` → 削除。Plan 側が Opus 本人であり、既に縮退した Plan から
  さらに落ちると Plan の質が読めなくなるので、黙って落とす取引はしない
- `fable/opus` → 最新 Sonnet。このモードが押しのけた実行側の家系であり、
  Plan(Fable)・実行(Opus)のどちらとも衝突しない。`sonnet` エイリアスを Opus で
  乗っ取っていても、`fallbackModel` は具体 ID なので本物の Sonnet に落ちる

## なぜ env なのか

エイリアスの解決は `ANTHROPIC_DEFAULT_<FAMILY>_MODEL` を**最優先**で読む。
claude 2.1.263 の実装(バイナリから抽出、名前は minify 後のもの):

```js
function $l(){let e=a.ANTHROPIC_DEFAULT_OPUS_MODEL;if(e!==void 0)return cS(e);return rl("opus")??en()}
function en(e=ec()){return nl("opus",e)??e.opus5}          // カタログ既定
function Mp(){let e=a.ANTHROPIC_DEFAULT_SONNET_MODEL;if(e!==void 0)return cS(e);return rl("sonnet")??Za()}
```

`opusplan` はこの 2 つを permission mode で振り分ける(Plan 側が `$l()`、実行側が
`Mp()`)。つまり `ANTHROPIC_DEFAULT_OPUS_MODEL` が Plan 側、
`ANTHROPIC_DEFAULT_SONNET_MODEL` が実行側のツマミであり、**モードはこの 2 つの
ペア**になる。Sonnet 実行のときは SONNET キーを書かない — カタログ既定がそのまま
望みの値であり、書けば「不在=未初期化」という番兵と、腐らない pin という性質の
両方を捨てることになる。

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

| `..._OPUS_MODEL` | `..._SONNET_MODEL` | `--model` | permission mode | 解決されたモデル |
|------------------|--------------------|-----------|-----------------|------------------|
| `claude-fable-5-1` | (不在) | `opusplan` | `plan` | `claude-fable-5-1[1m]` |
| `claude-fable-5-1` | (不在) | `opusplan` | `acceptEdits` | `claude-sonnet-5` |
| `claude-opus-5` | (不在) | `opusplan` | `plan` | `claude-opus-5[1m]` |
| `claude-fable-5-1` | `claude-opus-5` | `opusplan` | `plan` | `claude-fable-5-1[1m]` |
| `claude-fable-5-1` | `claude-opus-5` | `opusplan` | `acceptEdits` | `claude-opus-5` |
| `fable`(エイリアス文字列) | — | `opus` | — | **`unrecognized_model` エラー** |

下 2 行が `fable/opus` モードの実測で、Plan/実行の分離を保ったまま実行側だけが
Opus になっていることを示す。

Plan 側に `[1m]` が付くのは Opus Plan Mode 自身の挙動で、こちらで指定したもので
はない。Plan 中は 1M context で読める。

## リミットが枯れたときの手順

1. `claude-plan-model opus/sonnet` — 次に起動する claude から Opus Plan になる
   (引数なしの巡回でも辿り着くが、枯れているときは明示指定が速い)
2. 実行中のセッションは、次のどちらか
   - `claude --continue` で再起動する(**推奨**。opusplan の Plan/実行の分離を保てる)
   - `/model claude-opus-5` と直に指定する(再起動不要だが、そのセッションだけ
     Plan/実行の分離が消えてフラットな Opus になる)
3. Fable のリミット窓がリセットしたら `claude-plan-model fable/sonnet` で戻す

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

## 前提: `.model` が `"opusplan"` であること(書き込む前に落ちる)

この構成が効くのは `.model == "opusplan"`(または `"opusplan[1m]"`)のときだけ。
`.model` は `/model` で本人が日常的に切り替える対象なので、home-manager も
`claude-plan-model` も**書き換えない**。代わりに、`.model` を 3 つに分類して
**書き込む前に**判断する。

| 分類 | `.model` の値 | 何が起きるか | 挙動 |
|------|--------------|-------------|------|
| `split` | `opusplan` / `opusplan[1m]` | Plan/実行の分離が存在する | 無言で続行 |
| `alias` | `opus` / `opus[1m]` / `sonnet` / `sonnet[1m]` / `haiku` / `best` | **セッション全体**がそのエイリアス経由になる | 診断して exit 1 |
| `flat` | 具体モデル ID / `fable*` / 不在 | エイリアスを迂回するので本当に無効 | 促して exit 1 |

`alias` を独立させているのが要点。ここを「`opusplan` ではない → 無効」と一括で
扱うと、**嘘をつきながら全リクエストのモデルを書き換える**。例えば
`.model = "opus[1m]"` は `opus` エイリアスそのものなので、Plan 側だけでなく
セッション全体が Fable に化ける — かつてこの分類を持たず、しかも警告を
`apply_mode` の**後**に出していたため、「このスイッチは今 inert です」と言いながら
全体を Opus から Fable へ倒すという挙動になっていた。

`.model = "haiku"` も `alias` に入る。`haiku` は `(Plan 側 = sonnet エイリアス,
実行側 = haiku エイリアス)` という別のペアであり(`$me("haiku") === "sonnet"`)、
SONNET キーはその Plan 側を直撃する。逆に `fable` / `fable[1m]` は FABLE
エイリアス経由で、こちらが書かないキーなので `flat` 扱い。

意図的に強行したいときは `--force`。`sync` はこの判定を**しない** — activation は
本人の操作ではなく、実行時の選択(`.model`)を理由に `hms` を失敗させるのは筋が
違う。`status` は分類を報告するだけで exit 0 のまま。

## 副作用: 乗っ取ったエイリアスは全域に効く

差し替えの対象はエイリアスなので、Plan 側が Fable のモード中は `/model opus` を
選んでも実体は Fable になる。`opusplan` の Plan 側だけを狙い撃ちする方法は
存在しない(Plan 側の解決経路が `opus` エイリアスそのものだから)。素の Opus を
一時的に使いたいときはエイリアスを避けてモデル名を直接指定する:
`/model claude-opus-5`。

`fable/opus` では同じことが `sonnet` エイリアスに起きる。`/model sonnet` も、
`model: sonnet` を指定したサブエージェントも Opus で走る — **安いはずの並列
サブエージェントが Opus のリミットを食う**ので、このモードは「実装を Opus に
任せたい」ときに意図して選ぶものであり、常用の既定ではない。
`claude-plan-model` はこのモードに入るたびにその 1 行を出す。

`haiku` エイリアスはどのモードでも触らない。タイトル生成のような裏方処理が
Opus に化けないのは、この線を引いているからである。

## self-test

モードがペアになった時点で、面白い壊れ方は「片方だけ書き換わって相方が残る」
「ガードが書き込みの後に出る」「`sync` が意図的なモードを既定へ引き戻す」の
ような**組み合わせ**になった。そこで `claude-plan-model --selftest` が状態機械を
直接回す(CI の self-test 群に載せてある)。

テスト用のシームは 2 つだけ:

- `CLAUDE_PLAN_MODEL_SETTINGS` — 対象の settings.json(既存)
- `CLAUDE_PLAN_MODEL_CATALOG` — カタログ文字列。インストール済み claude の
  世代にテストが依存しないようにするため

確かめている不変条件: 未初期化から既定モードを seed する / 3 モードの巡回順と
一巡 / `fable/opus` を抜けるとき SONNET キーが消える / steady state では mtime
すら触らない / `.model` が `alias` でも `flat` でも**ファイルを触らずに** exit 1
し、`--force` では通る / 自分が書かない値(未知の Plan 値・具体 Sonnet ID)を
奪わない / `sync` が 3 モードそれぞれを保ったまま腐った ID を更新し、`.model` が
何であっても落ちない / 無関係なキーが保存される。

## 前提のバージョン依存性

上の関数名(`$l` / `Mp` / `en`)は minify 後の名前で、バージョンが上がれば変わる。
依存しているのは名前ではなく「エイリアス解決が `ANTHROPIC_DEFAULT_<FAMILY>_MODEL`
を最優先で読む」という公開された契約(env var 名とスキーマ)だけなので、実装の
内部名が変わっても設定は生き続ける。

一方 `latest_per_family` の抽出は minify されたリテラルに依存する。壊れても
settings.json は据え置かれ、警告だけが出る(pin が固まるだけで、設定が消えたり
壊れたりはしない)。挙動が怪しくなったら、上の `--settings` を使った手順で
実測を取り直すのが最短の確認手順。
