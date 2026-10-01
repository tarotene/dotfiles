//! bash 版 `copilot-plan-review.sh` の出力から生成した期待値(一時ディレクトリは
//! `<DIR>`、時刻は `<TS>`、mktemp の乱数は `<RND>` にマスク済み)。

pub fn get(name: &str) -> Option<&'static str> {
    Some(match name {
        "advisory_log" => {
            r####"## GATE: DENY

- ラウンド: 1 / lens: M
- readiness: R=NO S=yes I=yes T=NO
- open set: 2 件 (新規 2 / 未解消 0)
- backlog (MINOR/NIT): 2 件
- 不適合で破棄: 1 / 重複除去: 1 / 不正 severity: 0
- carry-over: 未応答 0 / 未知 id 1

## open set（ゲート対象）

### [BLOCKER][TECHNICAL] Dup  Summary
- id: R1-M-1 / 軸: IMPLEMENTATION
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1

### [MAJOR][NEEDS_DECISION] 人間の判断が要る
- id: R1-M-3 / 軸: SCOPE
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1

## 今ラウンドで解消 / 却下

（なし）

## backlog (MINOR/NIT)

### [MINOR][TECHNICAL] 命名
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1

### [NIT][TECHNICAL] 句点
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1

"####
        }
        "advisory_parallel_log" => {
            r####"## GATE: PASS

- ラウンド: 1 / lens: A
- readiness: R=yes S=yes I=yes T=yes
- open set: 0 件 (新規 0 / 未解消 0)
- backlog (MINOR/NIT): 0 件
- 不適合で破棄: 0 / 重複除去: 0 / 不正 severity: 0
- carry-over: 未応答 0 / 未知 id 0

## open set（ゲート対象）

（なし）

## 今ラウンドで解消 / 却下

（なし）

## backlog (MINOR/NIT)

（なし）

"####
        }
        "advisory_parallel_stderr" => {
            r####"（注意: lens B が失敗したため残りの critic のみで判定しています）
"####
        }
        "advisory_stderr" => {
            r####"（注意: 必須フィールドが空の BLOCKER/MAJOR 1 件を破棄しました。open set に無い carry-over id 1 件を無視しました）
"####
        }
        "backlog_r2" => {
            r####"## ラウンド 2

### [MINOR][TECHNICAL] 小さい話
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1
"####
        }
        "cap_escalate" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Copilot プランレビューの上限 (3 ラウンド) に到達しましたが、実装をブロックする指摘が 1 件未解消のまま残っています。\n\n追加のレビューは行いません。**AskUserQuestion で、この状態のまま実装に進んでよいか (GO / NO-GO) をユーザーに確認すること。** あなたの判断で未解消のまま進めてはならない。\n\n- GO なら、そのまま再度 ExitPlanMode を呼ぶ（次回は素通ります）。\n- NO-GO なら、ExitPlanMode を呼ばずにプランの修正を続けること。\n\n--- 未解消の指摘 ---\n\n### [BLOCKER][TECHNICAL] 前ラウンドの指摘\n- id: R1-A-1 / 軸: IMPLEMENTATION\n- 失敗モード: 壊れる\n- 発生条件: 常に\n- 根拠: a.sh:1\nMINOR/NIT は <DIR>/backlog/selftest-cap.md を参照。レビュー全文: <DIR>"
  }
}
"####
        }
        "cap_pass" => {
            r####"{
  "systemMessage": "Copilot プランレビュー: このセッションの上限 (3 ラウンド) に達したため素通しします。直近のレビュー: <DIR>"
}
"####
        }
        "closer_deny" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Copilot プランレビューの最終ラウンド (closer) で改訂プランに対して再判定した結果、実装をブロックする指摘が 1 件未解消のまま残っています。\n\n追加のレビューは行いません。**AskUserQuestion で、この状態のまま実装に進んでよいか (GO / NO-GO) をユーザーに確認すること。** あなたの判断で未解消のまま進めてはならない。\n\n- GO なら、そのまま再度 ExitPlanMode を呼ぶ（次回は素通ります）。\n- NO-GO なら、ExitPlanMode を呼ばずにプランの修正を続けること。\n\n--- 未解消の指摘 ---\n\n### [BLOCKER][TECHNICAL] 前ラウンドの指摘\n- id: R1-A-1 / 軸: IMPLEMENTATION\n- **前ラウンドから未解消**\n- 失敗モード: 壊れる\n- 発生条件: 常に\n- 根拠: a.sh:1\nMINOR/NIT は <DIR>/backlog/selftest-closer-deny.md を参照。レビュー全文: <DIR>/<TS>-selftest.md"
  }
}
"####
        }
        "closer_pass" => {
            r####"{
  "systemMessage": "Copilot プランレビュー: 前ラウンドの指摘はすべて解消 / 却下されました（closer ラウンド 3/3）。closer は carry-over 判定専用なので、新たに報告された BLOCKER/MAJOR 1 件（うち BLOCKER 0 件）は gate 対象にせず backlog に退避しました。実装前に <DIR>/backlog/selftest-closer-pass.md を確認してください。backlog は計 1 件。全文: <DIR>/<TS>-selftest.md"
}
"####
        }
        "critics_failed" => {
            r####"{
  "systemMessage": "Copilot プランレビュー: 実行に失敗しました（タイムアウト 20s・未ログイン・ネットワーク等）。fail-open で通過させます。ラウンドは消費していません。"
}
"####
        }
        "escalated_pass" => {
            r####"{
  "systemMessage": "Copilot プランレビュー: このセッションでは既に人間の GO/NO-GO を要求したため素通しします。直近のレビュー: <DIR>"
}
"####
        }
        "log_r2" => {
            r####"## GATE: DENY

- ラウンド: 2 / lens: C
- readiness: R=yes S=yes I=yes T=yes
- open set: 1 件 (新規 0 / 未解消 1)
- backlog (MINOR/NIT): 1 件
- 不適合で破棄: 0 / 重複除去: 0 / 不正 severity: 0
- carry-over: 未応答 0 / 未知 id 0

## open set（ゲート対象）

### [BLOCKER][TECHNICAL] 前ラウンドの指摘
- id: R1-A-1 / 軸: IMPLEMENTATION
- **前ラウンドから未解消**
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: a.sh:1

## 今ラウンドで解消 / 却下

（なし）

## backlog (MINOR/NIT)

### [MINOR][TECHNICAL] 小さい話
- 失敗モード: 壊れる
- 発生条件: 常に
- 根拠: evidence.md:1

"####
        }
        "max1_deny" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Copilot によるプランレビューの結果、実装をブロックする指摘が 1 件あります（ラウンド 1/1、新規 1 件 / 前ラウンドから未解消 0 件）。\n\n対応の方針:\n\n- **BLOCKER / MAJOR だけが対応対象**です。MINOR/NIT は <DIR>/backlog/selftest-max1.md に退避済みで、いま直す必要はありません。\n- [TECHNICAL] の指摘: リポジトリ等の証拠で検証し、妥当なら反映してください。**反証できた指摘は、反証の根拠をプランに明記して却下してよい**（却下は正当な帰結です）。\n- [NEEDS_DECISION] の指摘: 勝手に採否を判断してプランに反映してはいけません。レビュアーの意見はユーザーの決定ではありません。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てから修正してください。\n- 「もっと良いプランが存在する」ことは指摘理由になりません。任意の改善提案として書かれているものがあれば無視してよい。\n\n対応が済んでから再度 ExitPlanMode を呼んでください。次のラウンドでは、ここに挙がった各指摘が解消されたか / 反証されたか / 未解消かが判定されます。\n\n--- 未解消の指摘 ---\n\n### [BLOCKER][TECHNICAL] lens M の指摘\n- id: R1-M-1 / 軸: IMPLEMENTATION\n- 失敗モード: 壊れる\n- 発生条件: 常に\n- 根拠: evidence.md:1\nレビュー全文: <DIR>/<TS>-selftest.md"
  }
}
"####
        }
        "precheck_deny" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "(copilot-plan-review: plan-precedent-gate の書式 gate precheck による deny。このラウンドのレビューは未実行・未消費です)\n\nstub-deny-reason-12345"
  }
}
"####
        }
        "precheck_deny_scope_permission_request" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "deny",
      "message": "(copilot-plan-review: plan-scope-gate の書式 gate precheck による deny。このラウンドのレビューは未実行・未消費です)\n\nstub-deny-reason-12345"
    }
  }
}
"####
        }
        "prompt_lens_a" => {
            r####"あなたはシニアエンジニアとして、AI コーディングエージェント (Claude) が書いた実装プランをレビューする。

プラン本文: <DIR>/.plan.<RND>.md を読むこと。
作業ディレクトリは対象リポジトリである。プランの前提（ファイル・関数・設定の存在、既存パターンとの整合）を read-only で自由に探索して検証してよい。
プランが他のリポジトリやディレクトリ（例: ~/.ghr/github.com/ 配下の別リポジトリ、$HOME 直下の設定ファイル）を参照している場合は、それらも read-only で探索して検証してよい。ただしプランが参照していない場所の探索に迷い込まないこと。
ネットワークアクセスはできない。外部事実（API 仕様・ライブラリのバージョン・公式推奨）は、プランまたはリポジトリに記録された根拠だけで判定し、追加の web 検索は行わないこと。

## 受入基準

このレビューの目的は「完璧なプラン」を作ることではなく、プランが実装可能な状態
(implementation-ready) に達しているかを判定することである。

  Ready(P) = R かつ S かつ I かつ T
    R (requirements)    要件が特定されている
    S (scope)           スコープ / 非スコープが明確
    I (implementation)  実装手順が具体化されている
    T (verification)    検証方法が定義されている

「もっと良いプランが存在する」ことは指摘理由にならない。

## 報告してよいもの / いけないもの

報告するのは、実装をブロックする問題だけである。

報告してはいけない:
  - 同等な代替設計（どちらでも成立する設計上の選択）
  - 将来の拡張案・任意の改善提案
  - スタイル・命名・体裁の好み
  - 「〜した方がよい」で終わり、具体的な失敗を示せないもの

**findings が空配列であることは正常かつ望ましい結果である。**
指摘を捻り出す必要はない。ブロックすべき問題がなければ空配列を返せ。

## severity

  BLOCKER  このまま実装すると壊れる / 要件を満たさない
  MAJOR    実装前に決めないと手戻りが確実
  MINOR    直した方がよいが実装をブロックしない
  NIT      好み・体裁

BLOCKER と MAJOR だけがプラン修正を要求する。確信が持てないものは MINOR に落とせ。

## kind

  TECHNICAL       リポジトリや公式ドキュメントの証拠で白黒がつく欠陥
  NEEDS_DECISION  プラン作成者では決められず、人間のユーザーの判断・追加情報が必要

## 各 finding に必須のフィールド

  summary         一行要約
  failure_mode    具体的にどう失敗するか
  trigger         どういう条件で発生するか
  evidence        リポジトリ上の path[:line] / プランの該当箇所 / 外部仕様の出典
  readiness_axis  その指摘が壊す軸 (REQUIREMENTS / SCOPE / IMPLEMENTATION / VERIFICATION / NONE)

これらを具体的に埋められない指摘は報告しないこと。空文字や「不明」で埋めた BLOCKER /
MAJOR は機械的に破棄される（黙って通過扱いになる）。

## readiness

readiness の 4 boolean を出すこと。false にした軸は必ず BLOCKER または MAJOR の finding で
裏付けること。裏付けのない false は機械的に無視される。

## 出力

最終メッセージは以下の JSON にちょうど一致する 1 オブジェクトだけにすること。
コードフェンス(```)・前置き・要約・末尾のコメントは一切出力しない。
キーの過不足・型不一致・enum 外の値は機械的に破棄され、critic failure として
扱われる（黙って通過にはならないが、その critic の指摘は今ラウンドで失われる）。

  {
    "readiness": {
      "requirements": <boolean>, "scope": <boolean>,
      "implementation": <boolean>, "verification": <boolean>
    },
    "findings": [
      { "severity": "BLOCKER"|"MAJOR"|"MINOR"|"NIT",
        "kind": "TECHNICAL"|"NEEDS_DECISION",
        "readiness_axis": "REQUIREMENTS"|"SCOPE"|"IMPLEMENTATION"|"VERIFICATION"|"NONE",
        "summary": <string>, "failure_mode": <string>,
        "trigger": <string>, "evidence": <string> }, ...
    ],
    "carryover": [
      { "id": <string>, "status": "RESOLVED"|"UNRESOLVED"|"REFUTED_BY_PLAN",
        "rationale": <string> }, ...
    ]
  }

上記 3 キー(readiness/findings/carryover)以外は出力しないこと。各オブジェクトも
挙げたキーだけを持ち、余計なキーを足さないこと。

## このレビューの観点 (lens A)

要件 (R) とスコープ (S)、およびプランが置いている前提の誤りだけを見る。

  - プランが満たすべき要件が特定されているか。暗黙の要件を取り違えていないか
  - スコープと非スコープが切れているか。やると書いたことが実際にゴールを達成するか
  - プランが「存在する」と仮定しているファイル・関数・設定・挙動が実在するか
  - 参照している既存パターンや過去の決定と矛盾していないか

加えて `## 先行例との対比` 節(または `先行例: 該当なし` の免除行)を
監査する。節がある場合、各 `Dn` について:

  - 引用された出典(リポジトリ内パス・Issue/PR は実際に読むこと)が
    その設計判断を実際に支えているか
  - `差分: 異なる` の理由が成立するか。単なる好みの言い換えではないか
  - `先行例なし:` の探索範囲が、その設計判断の重要度に見合っているか
  - 節に載っていない非自明な設計判断が、プランの他所に紛れていないか

さらに技術・仕組みの選択(ツール・ライブラリの採否、hook/skill の要否)を
含む `Dn` は selection-grounding の観点でも監査する:

  - `軸:` の判定(表現不可能/還元/検出のみ)が実際の内容と整合するか。
    新しさそれ自体を表現不可能性や還元性の理由に流用していないか
  - 外部依存の新設・置換・撤去、または撤収コストが導入コストを上回る
    選択なのに、`本命:`/`対抗馬:` の重い欄が書かれていないか(発火漏れ。
    gate は節内整合性しか見ないため、発火判定はここでしか拾えない)
  - `対抗馬:` が本命と同じ評価軸で戦わされた本気の候補か。名前だけ挙げて
    比較していない体裁だけの対抗馬になっていないか
  - `外した候補:` の理由が実は感触なのに、分析的な理由で偽装していないか

**出典を示せない「もっと良い代替案があるかもしれない」型の指摘は
報告しないこと。** 理由のない逸脱は MAJOR、引用先行例と矛盾して要件を
満たさなくなる場合は BLOCKER。重い欄の発火漏れは MAJOR。免除行
(`先行例: 該当なし — <理由>`)が妥当かどうかは、プラン全体に非自明な
設計判断が本当にないかで判定する。

実装手順の粒度や検証方法の不足は別の critic が見るので、ここでは扱わない。

## carry-over

前ラウンドからの未解決指摘はない。carryover は空配列を返すこと。"####
        }
        "prompt_lens_c" => {
            r####"あなたはシニアエンジニアとして、AI コーディングエージェント (Claude) が書いた実装プランをレビューする。

プラン本文: <DIR>/.plan.<RND>.md を読むこと。
作業ディレクトリは対象リポジトリである。プランの前提（ファイル・関数・設定の存在、既存パターンとの整合）を read-only で自由に探索して検証してよい。
プランが他のリポジトリやディレクトリ（例: ~/.ghr/github.com/ 配下の別リポジトリ、$HOME 直下の設定ファイル）を参照している場合は、それらも read-only で探索して検証してよい。ただしプランが参照していない場所の探索に迷い込まないこと。
ネットワークアクセスはできない。外部事実（API 仕様・ライブラリのバージョン・公式推奨）は、プランまたはリポジトリに記録された根拠だけで判定し、追加の web 検索は行わないこと。

## 受入基準

このレビューの目的は「完璧なプラン」を作ることではなく、プランが実装可能な状態
(implementation-ready) に達しているかを判定することである。

  Ready(P) = R かつ S かつ I かつ T
    R (requirements)    要件が特定されている
    S (scope)           スコープ / 非スコープが明確
    I (implementation)  実装手順が具体化されている
    T (verification)    検証方法が定義されている

「もっと良いプランが存在する」ことは指摘理由にならない。

## 報告してよいもの / いけないもの

報告するのは、実装をブロックする問題だけである。

報告してはいけない:
  - 同等な代替設計（どちらでも成立する設計上の選択）
  - 将来の拡張案・任意の改善提案
  - スタイル・命名・体裁の好み
  - 「〜した方がよい」で終わり、具体的な失敗を示せないもの

**findings が空配列であることは正常かつ望ましい結果である。**
指摘を捻り出す必要はない。ブロックすべき問題がなければ空配列を返せ。

## severity

  BLOCKER  このまま実装すると壊れる / 要件を満たさない
  MAJOR    実装前に決めないと手戻りが確実
  MINOR    直した方がよいが実装をブロックしない
  NIT      好み・体裁

BLOCKER と MAJOR だけがプラン修正を要求する。確信が持てないものは MINOR に落とせ。

## kind

  TECHNICAL       リポジトリや公式ドキュメントの証拠で白黒がつく欠陥
  NEEDS_DECISION  プラン作成者では決められず、人間のユーザーの判断・追加情報が必要

## 各 finding に必須のフィールド

  summary         一行要約
  failure_mode    具体的にどう失敗するか
  trigger         どういう条件で発生するか
  evidence        リポジトリ上の path[:line] / プランの該当箇所 / 外部仕様の出典
  readiness_axis  その指摘が壊す軸 (REQUIREMENTS / SCOPE / IMPLEMENTATION / VERIFICATION / NONE)

これらを具体的に埋められない指摘は報告しないこと。空文字や「不明」で埋めた BLOCKER /
MAJOR は機械的に破棄される（黙って通過扱いになる）。

## readiness

readiness の 4 boolean を出すこと。false にした軸は必ず BLOCKER または MAJOR の finding で
裏付けること。裏付けのない false は機械的に無視される。

## 出力

最終メッセージは以下の JSON にちょうど一致する 1 オブジェクトだけにすること。
コードフェンス(```)・前置き・要約・末尾のコメントは一切出力しない。
キーの過不足・型不一致・enum 外の値は機械的に破棄され、critic failure として
扱われる（黙って通過にはならないが、その critic の指摘は今ラウンドで失われる）。

  {
    "readiness": {
      "requirements": <boolean>, "scope": <boolean>,
      "implementation": <boolean>, "verification": <boolean>
    },
    "findings": [
      { "severity": "BLOCKER"|"MAJOR"|"MINOR"|"NIT",
        "kind": "TECHNICAL"|"NEEDS_DECISION",
        "readiness_axis": "REQUIREMENTS"|"SCOPE"|"IMPLEMENTATION"|"VERIFICATION"|"NONE",
        "summary": <string>, "failure_mode": <string>,
        "trigger": <string>, "evidence": <string> }, ...
    ],
    "carryover": [
      { "id": <string>, "status": "RESOLVED"|"UNRESOLVED"|"REFUTED_BY_PLAN",
        "rationale": <string> }, ...
    ]
  }

上記 3 キー(readiness/findings/carryover)以外は出力しないこと。各オブジェクトも
挙げたキーだけを持ち、余計なキーを足さないこと。

## このレビューの観点 (lens C)

adversarial に見る。「このプランを実装した結果、何が壊れるか」を探す。

  - 手順どおりに実装したとき、既存の動作を壊す経路はどこか
  - エラー・タイムアウト・並行実行・部分失敗のときにどうなるか
  - プランが導入する新しい状態・ファイル・権限が、既存の前提と衝突しないか
  - プランが自分で塞いだつもりの穴が、実際には塞がっていない箇所はないか

新しい観点を無理に増やす必要はない。ブロックすべき問題がなければ findings は空配列でよい。

## carry-over（前ラウンドからの未解決指摘）

以下は前ラウンドでブロック要因と判定された指摘である。改訂されたプランを読み、
**各 id について carryover に必ず 1 件返すこと**。

  RESOLVED         改訂プランで解消された（rationale にプランのどこで解消されたかを書く）
  REFUTED_BY_PLAN  プランに書かれた反証が妥当で、指摘自体が誤りだった（rationale に理由）
  UNRESOLVED       まだ解消していない

判定を落とした id は保守的に UNRESOLVED として扱われる。落としても通過はしない。
同じ指摘を findings に再掲する必要はない。carryover で UNRESOLVED と答えれば足りる。

判定は **改訂後プランの現行本文** を根拠に述べること。下に添えた根拠は前ラウンド時点の
ものであり、改訂で行番号がずれたり記述そのものが消えていることがある。

  - 行番号の一致で照合するな。記述内容で照合せよ
  - 前ラウンドの根拠が指していた記述が現行プランに存在しないなら、その指摘は
    RESOLVED（書き換えで解消された）または REFUTED_BY_PLAN と判定せよ。
    存在しない記述を根拠に UNRESOLVED を返してはならない

- id: R1-A-1 [BLOCKER] lens A の指摘
  失敗モード: 壊れる
  根拠: evidence.md:1"####
        }
        "prompt_lens_z" => {
            r####"あなたはシニアエンジニアとして、AI コーディングエージェント (Claude) が書いた実装プランをレビューする。

プラン本文: <DIR>/.plan.<RND>.md を読むこと。
作業ディレクトリは対象リポジトリである。プランの前提（ファイル・関数・設定の存在、既存パターンとの整合）を read-only で自由に探索して検証してよい。
プランが他のリポジトリやディレクトリ（例: ~/.ghr/github.com/ 配下の別リポジトリ、$HOME 直下の設定ファイル）を参照している場合は、それらも read-only で探索して検証してよい。ただしプランが参照していない場所の探索に迷い込まないこと。
ネットワークアクセスはできない。外部事実（API 仕様・ライブラリのバージョン・公式推奨）は、プランまたはリポジトリに記録された根拠だけで判定し、追加の web 検索は行わないこと。

## 受入基準

このレビューの目的は「完璧なプラン」を作ることではなく、プランが実装可能な状態
(implementation-ready) に達しているかを判定することである。

  Ready(P) = R かつ S かつ I かつ T
    R (requirements)    要件が特定されている
    S (scope)           スコープ / 非スコープが明確
    I (implementation)  実装手順が具体化されている
    T (verification)    検証方法が定義されている

「もっと良いプランが存在する」ことは指摘理由にならない。

## 報告してよいもの / いけないもの

報告するのは、実装をブロックする問題だけである。

報告してはいけない:
  - 同等な代替設計（どちらでも成立する設計上の選択）
  - 将来の拡張案・任意の改善提案
  - スタイル・命名・体裁の好み
  - 「〜した方がよい」で終わり、具体的な失敗を示せないもの

**findings が空配列であることは正常かつ望ましい結果である。**
指摘を捻り出す必要はない。ブロックすべき問題がなければ空配列を返せ。

## severity

  BLOCKER  このまま実装すると壊れる / 要件を満たさない
  MAJOR    実装前に決めないと手戻りが確実
  MINOR    直した方がよいが実装をブロックしない
  NIT      好み・体裁

BLOCKER と MAJOR だけがプラン修正を要求する。確信が持てないものは MINOR に落とせ。

## kind

  TECHNICAL       リポジトリや公式ドキュメントの証拠で白黒がつく欠陥
  NEEDS_DECISION  プラン作成者では決められず、人間のユーザーの判断・追加情報が必要

## 各 finding に必須のフィールド

  summary         一行要約
  failure_mode    具体的にどう失敗するか
  trigger         どういう条件で発生するか
  evidence        リポジトリ上の path[:line] / プランの該当箇所 / 外部仕様の出典
  readiness_axis  その指摘が壊す軸 (REQUIREMENTS / SCOPE / IMPLEMENTATION / VERIFICATION / NONE)

これらを具体的に埋められない指摘は報告しないこと。空文字や「不明」で埋めた BLOCKER /
MAJOR は機械的に破棄される（黙って通過扱いになる）。

## readiness

readiness の 4 boolean を出すこと。false にした軸は必ず BLOCKER または MAJOR の finding で
裏付けること。裏付けのない false は機械的に無視される。

## 出力

最終メッセージは以下の JSON にちょうど一致する 1 オブジェクトだけにすること。
コードフェンス(```)・前置き・要約・末尾のコメントは一切出力しない。
キーの過不足・型不一致・enum 外の値は機械的に破棄され、critic failure として
扱われる（黙って通過にはならないが、その critic の指摘は今ラウンドで失われる）。

  {
    "readiness": {
      "requirements": <boolean>, "scope": <boolean>,
      "implementation": <boolean>, "verification": <boolean>
    },
    "findings": [
      { "severity": "BLOCKER"|"MAJOR"|"MINOR"|"NIT",
        "kind": "TECHNICAL"|"NEEDS_DECISION",
        "readiness_axis": "REQUIREMENTS"|"SCOPE"|"IMPLEMENTATION"|"VERIFICATION"|"NONE",
        "summary": <string>, "failure_mode": <string>,
        "trigger": <string>, "evidence": <string> }, ...
    ],
    "carryover": [
      { "id": <string>, "status": "RESOLVED"|"UNRESOLVED"|"REFUTED_BY_PLAN",
        "rationale": <string> }, ...
    ]
  }

上記 3 キー(readiness/findings/carryover)以外は出力しないこと。各オブジェクトも
挙げたキーだけを持ち、余計なキーを足さないこと。

## このレビューの観点 (lens Z — closer)

これはこのセッションの最終ラウンドである。あなたの職責は **carry-over の判定だけ** で
あり、それ以外にはない。

  - 新しい欠陥を探すな。プランを読み直して別の問題を見つけ出す作業はしない
  - 下の carry-over 節にある各 id について、改訂プランを読んで判定を返すことに専念せよ
  - **findings は空配列を返せ。** ここで報告した新規指摘は severity に関わらず
    gate 対象にならず backlog に退避されるだけなので、探す労力に見合わない

carry-over の判定精度がこのラウンドの唯一の成果物である。

## carry-over（前ラウンドからの未解決指摘）

以下は前ラウンドでブロック要因と判定された指摘である。改訂されたプランを読み、
**各 id について carryover に必ず 1 件返すこと**。

  RESOLVED         改訂プランで解消された（rationale にプランのどこで解消されたかを書く）
  REFUTED_BY_PLAN  プランに書かれた反証が妥当で、指摘自体が誤りだった（rationale に理由）
  UNRESOLVED       まだ解消していない

判定を落とした id は保守的に UNRESOLVED として扱われる。落としても通過はしない。
同じ指摘を findings に再掲する必要はない。carryover で UNRESOLVED と答えれば足りる。

判定は **改訂後プランの現行本文** を根拠に述べること。下に添えた根拠は前ラウンド時点の
ものであり、改訂で行番号がずれたり記述そのものが消えていることがある。

  - 行番号の一致で照合するな。記述内容で照合せよ
  - 前ラウンドの根拠が指していた記述が現行プランに存在しないなら、その指摘は
    RESOLVED（書き換えで解消された）または REFUTED_BY_PLAN と判定せよ。
    存在しない記述を根拠に UNRESOLVED を返してはならない

- id: R1-A-1 [BLOCKER] 前ラウンドの指摘
  失敗モード: 壊れる
  根拠: a.sh:1"####
        }
        "round1_deny" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Copilot によるプランレビューの結果、実装をブロックする指摘が 1 件あります（ラウンド 1/3、新規 1 件 / 前ラウンドから未解消 0 件）。\n\n対応の方針:\n\n- **BLOCKER / MAJOR だけが対応対象**です。MINOR/NIT は <DIR>/backlog/selftest-parallel.md に退避済みで、いま直す必要はありません。\n- [TECHNICAL] の指摘: リポジトリ等の証拠で検証し、妥当なら反映してください。**反証できた指摘は、反証の根拠をプランに明記して却下してよい**（却下は正当な帰結です）。\n- [NEEDS_DECISION] の指摘: 勝手に採否を判断してプランに反映してはいけません。レビュアーの意見はユーザーの決定ではありません。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てから修正してください。\n- 「もっと良いプランが存在する」ことは指摘理由になりません。任意の改善提案として書かれているものがあれば無視してよい。\n\n対応が済んでから再度 ExitPlanMode を呼んでください。次のラウンドでは、ここに挙がった各指摘が解消されたか / 反証されたか / 未解消かが判定されます。\n\n（注意: lens B が失敗したため残りの critic のみで判定しました）\n\n--- 未解消の指摘 ---\n\n### [BLOCKER][TECHNICAL] lens A の指摘\n- id: R1-A-1 / 軸: IMPLEMENTATION\n- 失敗モード: 壊れる\n- 発生条件: 常に\n- 根拠: evidence.md:1\nレビュー全文: <DIR>/<TS>-selftest.md"
  }
}
"####
        }
        "round2_deny" => {
            r####"{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "Copilot によるプランレビューの結果、実装をブロックする指摘が 1 件あります（ラウンド 2/3、新規 0 件 / 前ラウンドから未解消 1 件）。\n\n対応の方針:\n\n- **BLOCKER / MAJOR だけが対応対象**です。MINOR/NIT は <DIR>/backlog/selftest-r2.md に退避済みで、いま直す必要はありません。\n- [TECHNICAL] の指摘: リポジトリ等の証拠で検証し、妥当なら反映してください。**反証できた指摘は、反証の根拠をプランに明記して却下してよい**（却下は正当な帰結です）。\n- [NEEDS_DECISION] の指摘: 勝手に採否を判断してプランに反映してはいけません。レビュアーの意見はユーザーの決定ではありません。必ず AskUserQuestion でユーザーに論点と選択肢（あなたの推奨付き）を提示し、回答を得てから修正してください。\n- 「もっと良いプランが存在する」ことは指摘理由になりません。任意の改善提案として書かれているものがあれば無視してよい。\n\n対応が済んでから再度 ExitPlanMode を呼んでください。次のラウンドでは、ここに挙がった各指摘が解消されたか / 反証されたか / 未解消かが判定されます。\n\n--- 未解消の指摘 ---\n\n### [BLOCKER][TECHNICAL] 前ラウンドの指摘\n- id: R1-A-1 / 軸: IMPLEMENTATION\n- **前ラウンドから未解消**\n- 失敗モード: 壊れる\n- 発生条件: 常に\n- 根拠: a.sh:1\nレビュー全文: <DIR>/<TS>-selftest.md"
  }
}
"####
        }
        "round2_pass" => {
            r####"{
  "systemMessage": "Copilot プランレビュー: 実装をブロックする指摘はありません（ラウンド 2/3）。MINOR/NIT 0 件は <DIR>/backlog/selftest-parallel.md に退避しました。全文: <DIR>/<TS>-selftest.md"
}
"####
        }
        _ => return None,
    })
}
