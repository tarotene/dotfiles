//! Stop hook の統合テスト(旧 `pr-gate.sh --selftest` の Stop 側ケース)。
//! 各 `// ok …` コメントが旧 selftest の `check` / `check_grep` 1 件に対応する。

mod common;
use common::*;

fn has(hay: &str, needle: &str) -> bool {
    hay.contains(needle)
}

// --- 対象外(完全沈黙) ---

#[test]
fn outside_allowlist_is_silent() {
    let fx = Fx::new();
    let other = fx.dir().join("other-repo");
    std::fs::create_dir_all(&other).unwrap();
    fx.git_in(&other, &["init", "-q"]);
    fx.git_in(
        &other,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/other/other.git",
        ],
    );
    let r = fx.run_in(
        &other,
        "stop",
        &format!(r#"{{"cwd":"{}"}}"#, other.display()),
        &[],
    );
    assert_eq!(r.code, 0); // ok   allowlist 外(stop): exit 0
    assert_eq!(r.stdout, ""); // ok   allowlist 外(stop): stdout 空
    assert_eq!(r.stderr, ""); // ok   allowlist 外(stop): stderr 空
}

#[test]
fn gh_missing_is_silent() {
    let mut fx = Fx::new();
    let nogh = fx.dir().join("nogh");
    std::fs::create_dir_all(&nogh).unwrap();
    for c in [
        "jq", "git", "awk", "sed", "grep", "basename", "dirname", "cat", "wc", "tr", "stat",
        "sleep", "date", "timeout", "mktemp", "find", "chmod", "mkdir",
    ] {
        let _ = std::os::unix::fs::symlink(which(c), nogh.join(c));
    }
    fx.path = nogh.display().to_string();
    let r = fx.stop("selftest-sid", &[]);
    assert_eq!(r.code, 0); // ok   gh 不在(stop): exit 0
    assert_eq!(r.all(), ""); // ok   gh 不在(stop): 出力なし
}

// --- G_push ---

#[test]
fn g_push_blocks_unpushed_head() {
    let fx = Fx::new();
    let r = fx.stop("selftest-sid", &[("PR_GATE_STUB_HEAD_OID", ZERO_OID)]);
    assert_eq!(r.code, 2); // ok   未push で block: exit 2
    assert!(has(&r.stderr, "未 push")); // ok   未push で block: メッセージに '未 push'
    assert!(has(&r.stderr, "bleep が deny")); // ok   未push で block: pre-push 迂回は bleep が deny する旨
}

// --- G_unpushed (PR がまだ無いときの push 忘れ) ---

#[test]
fn g_unpushed_blocks_when_commits_not_on_remote() {
    let fx = Fx::new();
    fx.set_upstream(&fx.rev("HEAD~2"));
    let r = fx.stop("gu-behind-sid", &[("PR_GATE_STUB_NO_PR", "1")]);
    assert_eq!(r.code, 2); // ok   PR 無し + 未push commit あり: exit 2
    assert!(has(&r.stderr, "未 push の commit が 2 件")); // ok   PR 無し + 未push: 件数が出る
    assert!(has(&r.stderr, "gh pr create")); // ok   PR 無し + 未push: push と PR 作成まで促す
}

#[test]
fn g_unpushed_silent_when_all_pushed() {
    let fx = Fx::new();
    fx.set_upstream(&fx.real_head);
    let r = fx.stop("gu-clean-sid", &[("PR_GATE_STUB_NO_PR", "1")]);
    assert_eq!(r.code, 0); // ok   PR 無し + 未push commit 0 件: exit 0
    assert_eq!(r.all(), ""); // ok   PR 無し + 未push commit 0 件: 出力なし
}

// --- G_pr (push 済み・PR 無し・ahead>0 の block) ---
// default branch 名に "main" を使うと、init.defaultBranch が "main" の環境で
// 現在のブランチと一致し、branch==default_br の skip 条件で全ケースが素通りして
// しまう。専用名 gate-test-default にする。

fn gpr_fixture() -> Fx {
    let fx = Fx::new();
    fx.set_origin_head("gate-test-default", Some(&fx.rev("HEAD~2")));
    fx.set_upstream(&fx.real_head);
    fx
}

#[test]
fn g_pr_blocks_pushed_branch_without_pr() {
    let fx = gpr_fixture();
    let r = fx.stop("gpr-block-sid", &[("PR_GATE_STUB_NO_PR", "1")]);
    assert_eq!(r.code, 2); // ok   push 済み+PR無し+ahead>0: exit 2
    assert!(has(&r.stderr, "gh pr create")); // ok   push済み+PR無し: gh pr create を促す
    assert!(has(&r.stderr, "pr-description")); // ok   push済み+PR無し: pr-description を案内
}

#[test]
fn g_pr_silent_when_merged_pr_exists() {
    let fx = gpr_fixture();
    let r = fx.stop(
        "gpr-merged-sid",
        &[
            ("PR_GATE_STUB_NO_PR", "1"),
            ("PR_GATE_STUB_ALL_STATE", "MERGED"),
        ],
    );
    assert_eq!(r.code, 0); // ok   merged PR 既存: exit 0(squash 残骸を block しない)
    assert_eq!(r.all(), ""); // ok   merged PR 既存: 出力なし
}

#[test]
fn g_pr_silent_without_origin_head() {
    let fx = gpr_fixture();
    fx.git(&["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    let r = fx.stop("gpr-nodefault-sid", &[("PR_GATE_STUB_NO_PR", "1")]);
    assert_eq!(r.code, 0); // ok   origin/HEAD 不明: exit 0(判定不能を断定に変えない)
    assert_eq!(r.all(), ""); // ok   origin/HEAD 不明: 出力なし
}

#[test]
fn g_pr_silent_when_ahead_zero() {
    let fx = gpr_fixture();
    fx.set_origin_head("gate-test-default", Some(&fx.real_head));
    let r = fx.stop("gpr-ahead0-sid", &[("PR_GATE_STUB_NO_PR", "1")]);
    assert_eq!(r.code, 0); // ok   ahead 0: exit 0(PR に値する commit が無い)
    assert_eq!(r.all(), ""); // ok   ahead 0: 出力なし
}

// --- G_CI ---

fn ci(
    fx: &Fx,
    sid: &str,
    rules: Option<&str>,
    checks: &str,
    extra: &[(&str, &str)],
) -> common::Run {
    let checks = fx.write(&format!("{sid}-checks.json"), checks);
    let mut envs: Vec<(&str, String)> = vec![
        ("PR_GATE_STUB_HEAD_OID", fx.real_head.clone()),
        ("PR_GATE_STUB_CHECKS_FILE", checks),
    ];
    if let Some(r) = rules {
        envs.push((
            "PR_GATE_STUB_RULES_FILE",
            fx.write(&format!("{sid}-rules.json"), r),
        ));
    }
    let mut all: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    all.extend_from_slice(extra);
    fx.stop(sid, &all)
}

#[test]
fn g_ci_all_required_pass() {
    // G_CI (PASS 経路の到達可能性 — #26 の回帰対象)
    let fx = Fx::new();
    let r = ci(&fx, "pass-sid", Some(RULES_2), CHECKS_2PASS, &[]);
    assert_eq!(r.code, 0); // ok   push 済み + 全 required pass: 素通り(exit 0)
}

#[test]
fn g_ci_rules_api_failure_blocks() {
    // G_CI (required 無しと API 障害を区別する — #135 の回帰対象)
    let fx = Fx::new();
    let r = ci(
        &fx,
        "apifail-sid",
        None,
        CHECKS_2PASS,
        &[("PR_GATE_STUB_RULES_FAIL", "1")],
    );
    assert_eq!(r.code, 2); // ok   rules/branches の gh api 失敗: exit 2(quiesce に縮退しない)
    assert!(has(&r.stderr, "取得に失敗")); // ok   API 障害メッセージ
}

#[test]
fn g_ci_no_checks_blocks() {
    let fx = Fx::new();
    let r = ci(&fx, "empty-sid", None, "[]", &[]);
    assert_eq!(r.code, 2); // ok   チェック0件: exit 2
    assert!(has(&r.stderr, "1 件も報告されていません")); // ok   チェック0件: メッセージ
}

#[test]
fn g_ci_missing_required_blocks() {
    let fx = Fx::new();
    let r = ci(&fx, "missing-sid", Some(RULES_2), CHECKS_1OF2, &[]);
    assert_eq!(r.code, 2); // ok   2件中1件しか出現していない: exit 2
    assert!(has(&r.stderr, "job-b")); // ok   未出現の job-b が列挙される
}

#[test]
fn g_ci_partial_pending_blocks_even_if_watch_exits_zero() {
    // R2-C-1 / R2-C-2 の核心: --watch が exit 0 を返しても、判定に使う --json では
    // job-b がまだ pending。判定が --watch の exit code に依存していたら素通りする。
    let fx = Fx::new();
    let r = ci(
        &fx,
        "partial-sid",
        Some(RULES_2),
        CHECKS_PARTIAL_PENDING,
        &[("PR_GATE_STUB_WATCH_RC", "0")],
    );
    assert_eq!(r.code, 2); // ok   --watch が exit 0 でも部分 pending は block される(--watch 非依存の検査)
    assert!(has(&r.stderr, "pending")); // ok   pending メッセージ
}

#[test]
fn g_ci_failed_job_blocks_with_log_command() {
    let fx = Fx::new();
    let r = ci(
        &fx,
        "failsid",
        Some(RULES_2),
        CHECKS_PARTIAL_FAIL,
        &[("PR_GATE_STUB_WATCH_RC", "0")],
    );
    assert_eq!(r.code, 2); // ok   job-b が fail: exit 2
    assert!(has(&r.stderr, "job-b")); // ok   失敗ジョブ名が出る
    assert!(has(&r.stderr, "gh run view 1 --log-failed --job 12")); // ok   gh run view コマンドが出る
}

#[test]
fn g_ci_quiesce_pass_with_note() {
    // G_CI (stacked PR: required 0件 → quiesce フォールバック)
    let fx = Fx::new();
    let r = ci(
        &fx,
        "quiesce-pass-sid",
        Some("[]"),
        CHECKS_QUIESCE_PASS,
        &[],
    );
    assert_eq!(r.code, 0); // ok   stacked PR + 全チェック pass: 素通り(exit 0)
    assert!(has(&r.stderr, "ruleset の対象外")); // ok   stacked PR: 注記が出る
}

#[test]
fn g_ci_quiesce_fail_blocks() {
    let fx = Fx::new();
    let r = ci(
        &fx,
        "quiesce-fail-sid",
        Some("[]"),
        CHECKS_QUIESCE_FAIL,
        &[],
    );
    assert_eq!(r.code, 2); // ok   stacked PR + 1件 fail: exit 2
}

// --- G_link (PR 本文 → Issue のリンク) ---
// push 済み + 全 required pass、つまり「あとは終わるだけ」の状態で回す。

#[test]
fn g_link_missing_blocks_and_merges_with_visual() {
    let fx = Fx::new();
    let r = fx.glink("link-missing-sid", "本文に Issue への言及が一切ない PR。");
    assert_eq!(r.code, 2); // ok   closing keyword も No-Issue: も無い: exit 2
    assert!(has(&r.stderr, "Closes #<番号>")); // ok   block メッセージが Closes を教える
    assert!(has(&r.stderr, "No-Issue: <理由>")); // ok   block メッセージが No-Issue: を教える
                                                 // この body は視覚証跡も無いので、G_link と G_visual が 1 つの block に合流する。
    assert!(has(&r.stderr, "gh pr edit")); // ok   同じ block に --attach の案内も乗る
    assert!(has(&r.stderr, "No-Visual: <理由>")); // ok   同じ block に No-Visual: の案内も乗る
}

#[test]
fn g_link_mention_without_keyword_blocks() {
    // #28/#29 を実質解決した PR は Issue に一切言及していなかった。
    let fx = Fx::new();
    let r = fx.glink(
        "link-mention-only-sid",
        "関連: #30 の調査で見つけた問題を直す。",
    );
    assert_eq!(r.code, 2); // ok   番号への言及はあるが keyword が無い: exit 2
}

#[test]
fn g_link_accepted_forms() {
    let fx = Fx::new();
    let cases = [
        // ok   Closes #N: 素通り(exit 0)
        ("link-closes-sid", "本文。\n\nCloses #30\nNo-Visual: selftest"),
        // ok   大文字 + コロン形式も GitHub の仕様どおり受理
        ("link-colon-sid", "CLOSES: #30\nNo-Visual: selftest"),
        // ok   クロスリポジトリ形式を受理
        ("link-crossrepo-sid", "Fixes octo-org/octo-repo#100\nNo-Visual: selftest"),
        // ok   issue URL 形式を受理
        (
            "link-url-sid",
            "Resolves https://github.com/example/example/issues/7\nNo-Visual: selftest",
        ),
        // ok   No-Issue: + 理由: 素通り(exit 0)
        (
            "link-noissue-sid",
            "No-Issue: セッション中に生まれた作業で対応 Issue が無い\nNo-Visual: selftest",
        ),
        // ok   コード外に本物があれば LINKED(exit 0)
        (
            "link-fence-plus-real-sid",
            "規約の例:\n\n```\nCloses #99\n```\n\nCloses #30\nNo-Visual: selftest",
        ),
        // ok   コード内 keyword + No-Issue: は No-Issue: が効く(exit 0)
        (
            "link-code-then-noissue-sid",
            "`Closes #99` の書き方を説明する PR。\n\nNo-Issue: 規約を説明するだけで対応 Issue は無い\nNo-Visual: selftest",
        ),
    ];
    for (sid, body) in cases {
        let r = fx.glink(sid, body);
        assert_eq!(r.code, 0, "{sid}: {}", r.stderr);
    }
}

#[test]
fn g_link_keyword_list_blocks_and_names_the_dropped_refs() {
    // `Closes #30 #31` は #30 しか閉じない(1 keyword 1 Issue、#722)。
    // keyword 自体はあるので「なし」とは別の理由で block する。
    let fx = Fx::new();
    let r = fx.glink("link-list-sid", "Closes #30 #31\nNo-Visual: selftest");
    assert_eq!(r.code, 2, "{}", r.stderr);
    assert!(has(&r.stderr, "番号を並べた書き方")); // ok   理由を名指しする
    assert!(has(&r.stderr, "#31")); // ok   閉じられない参照を挙げる
    assert!(!has(&r.stderr, "closing keyword なし")); // ok   「keyword なし」とは区別する
}

#[test]
fn g_link_rejected_forms() {
    let fx = Fx::new();
    let cases = [
        // ok   理由の無い No-Issue: は逃がさない(exit 2)
        ("link-noissue-bare-sid", "No-Issue:"),
        // ok   インラインのコードスパン内の keyword は数えない(exit 2)
        (
            "link-inline-code-sid",
            "本文に `Closes #30` と書く規約を説明する PR。",
        ),
        // ok   fenced code block 内の keyword は数えない(exit 2)
        (
            "link-fence-sid",
            "規約の例:\n\n```\nCloses #30\n```\n\n説明の続き。",
        ),
    ];
    for (sid, body) in cases {
        let r = fx.glink(sid, body);
        assert_eq!(r.code, 2, "{sid}: {}", r.stderr);
    }
}

#[test]
fn g_link_and_visual_ride_on_g_push() {
    // G_push が止める場面では、本文の指摘は単独 block ではなく相乗りで伝える。
    let fx = Fx::new();
    let r = fx.stop(
        "link-rider-sid",
        &[
            ("PR_GATE_STUB_HEAD_OID", ZERO_OID),
            ("PR_GATE_STUB_PR_BODY", "言及なし"),
        ],
    );
    assert_eq!(r.code, 2); // ok   G_push block 時: exit 2
    assert!(has(&r.stderr, "closing keyword なし")); // ok   G_push block に G_link が相乗りする
    assert!(has(&r.stderr, "視覚証跡がありません")); // ok   G_push block に G_visual も相乗りする
    assert!(has(&r.stderr, "未 push")); // ok   G_push の指摘も残る
}

fn offbase(fx: &Fx, sid: &str) -> common::Run {
    ci(
        fx,
        sid,
        Some("[]"),
        CHECKS_QUIESCE_PASS,
        &[
            ("PR_GATE_STUB_BASE", "stacked-base"),
            ("PR_GATE_STUB_PR_BODY", "Closes #30\nNo-Visual: selftest"),
        ],
    )
}

#[test]
fn g_link_offbase_advisory() {
    // closing keyword は default branch へのマージでのみ発火する(GitHub 公式)。
    let fx = Fx::new();
    fx.set_origin_head("main", None);
    let r = offbase(&fx, "link-offbase-sid");
    assert_eq!(r.code, 0); // ok   base が default branch でない: block はしない(exit 0)
    assert!(has(&r.stderr, "発火しません")); // ok   発火しない旨の advisory が出る
}

#[test]
fn g_link_offbase_without_origin_head_is_quiet() {
    let fx = Fx::new();
    let r = offbase(&fx, "link-nohead-sid");
    assert_eq!(r.code, 0); // ok   origin/HEAD 不明: exit 0
    assert!(!has(&r.stderr, "発火しません")); // ok   origin/HEAD 不明: 発火しない旨は出さない
}

// --- 中断ハンドオフ(Handoff: #N — G_link / G_CI の緩和) ---
// CI が揃っていない(チェック 0 件、G_CI 単独なら block)状態で回し、Handoff
// 成立時だけそれが advisory に落ちることを検査する。

type Envs<'a> = &'a [(&'a str, &'a str)];

const HANDOFF_BODY: &str = "作業ログ。\n\nHandoff: #1\nNo-Visual: selftest";

fn ghandoff(fx: &Fx, sid: &str, body: &str, extra: &[(&str, &str)]) -> common::Run {
    let mut envs = vec![("PR_GATE_STUB_PR_BODY", body)];
    envs.extend_from_slice(extra);
    ci(fx, sid, None, "[]", &envs)
}

#[test]
fn handoff_draft_open_relaxes_link_and_ci() {
    let fx = Fx::new();
    let r = ghandoff(
        &fx,
        "handoff-ok-sid",
        HANDOFF_BODY,
        &[("PR_GATE_STUB_DRAFT", "true")],
    );
    assert_eq!(r.code, 0); // ok   draft+Handoff+open: 素通り(exit 0)
    assert!(has(&r.stderr, "中断")); // ok   closing keyword 省略を許容する advisory
    assert!(has(&r.stderr, "G_CI: EMPTY")); // ok   G_CI も advisory に落ちる
}

#[test]
fn handoff_not_relaxed_cases() {
    let fx = Fx::new();
    let fenced = "本文。\n\nCloses #30\nNo-Visual: selftest\n\n```\nHandoff: #1\n```";
    let cases: [(&str, &str, Envs); 5] = [
        // ok   非 draft + Handoff: 緩和されず block(exit 2)
        ("handoff-nondraft-sid", HANDOFF_BODY, &[]),
        // ok   draft+Handoff+参照先が closed: 判定不能扱いで block(exit 2)
        (
            "handoff-closed-sid",
            HANDOFF_BODY,
            &[
                ("PR_GATE_STUB_DRAFT", "true"),
                ("PR_GATE_STUB_HANDOFF_STATE", "CLOSED"),
            ],
        ),
        // ok   draft+Handoff+gh issue view 失敗: fail-closed で block(exit 2)
        (
            "handoff-apifail-sid",
            HANDOFF_BODY,
            &[
                ("PR_GATE_STUB_DRAFT", "true"),
                ("PR_GATE_STUB_HANDOFF_FAIL", "1"),
            ],
        ),
        // ok   fenced code block 内の Handoff は数えない(G_CI が block、exit 2)
        (
            "handoff-fenced-sid",
            fenced,
            &[("PR_GATE_STUB_DRAFT", "true")],
        ),
        // ok   draft だが Handoff 行自体が無い: G_link が block(exit 2)
        (
            "handoff-none-sid",
            "本文にはどの Issue への言及も無い。",
            &[("PR_GATE_STUB_DRAFT", "true")],
        ),
    ];
    for (sid, body, extra) in cases {
        let r = ghandoff(&fx, sid, body, extra);
        assert_eq!(r.code, 2, "{sid}: {}", r.stderr);
    }
}

// --- G_visual (PR 本文の Before/After 視覚証跡) ---
// body には常に Closes #1 を含め、G_link 側を LINKED に固定して切り出す。

#[test]
fn g_visual_accepted_forms() {
    let fx = Fx::new();
    let cases = [
        // ok   user-attachments 画像: 素通り(exit 0)
        (
            "visual-image-sid",
            "Closes #1\n\n## Before / After\n\n![After](https://github.com/user-attachments/assets/abc123-def456)",
        ),
        // ok   Before/After 見出し配下の fenced code block: 素通り(exit 0)
        (
            "visual-codeblock-sid",
            "Closes #1\n\n## Before / After\n\n```\n- before: 3 columns\n+ after:  4 columns\n```",
        ),
        // ok   No-Visual: + 理由: 素通り(exit 0)
        ("visual-noissue-sid", "Closes #1\nNo-Visual: 外観に影響しない内部リファクタ"),
    ];
    for (sid, body) in cases {
        let r = fx.glink(sid, body);
        assert_eq!(r.code, 0, "{sid}: {}", r.stderr);
    }
}

#[test]
fn g_visual_missing_blocks_with_guidance() {
    let fx = Fx::new();
    let r = fx.glink("visual-missing-sid", "Closes #1");
    assert_eq!(r.code, 2); // ok   証跡が何も無い: exit 2
    assert!(has(&r.stderr, "gh pr edit")); // ok   証跡なし: --attach の案内が出る
    assert!(has(&r.stderr, "No-Visual: <理由>")); // ok   証跡なし: No-Visual: の案内が出る
}

#[test]
fn g_visual_rejected_forms() {
    let fx = Fx::new();
    let cases = [
        // ok   理由の無い No-Visual: は逃がさない(exit 2)
        ("visual-noissue-bare-sid", "Closes #1\nNo-Visual:"),
        // ok   インラインのコードスパン内の画像記法は数えない(exit 2)
        (
            "visual-inline-code-sid",
            "Closes #1\n\n## Before / After\n\n画像記法の例: `![After](https://github.com/user-attachments/assets/abc)` の形で貼る。",
        ),
        // ok   Before/After 以外の見出し配下の fence は数えない(exit 2)
        (
            "visual-wrong-heading-sid",
            "Closes #1\n\n## 検証\n\n```\n$ nix flake check\n...\n```",
        ),
        // ok   ローカルパスの画像記法はアップロード未証明として扱う(exit 2)
        (
            "visual-local-path-sid",
            "Closes #1\n\n## Before / After\n\n![Before](./before.png)\n![After](./after.png)",
        ),
    ];
    for (sid, body) in cases {
        let r = fx.glink(sid, body);
        assert_eq!(r.code, 2, "{sid}: {}", r.stderr);
    }
}

// --- G_stack (stacked PR チェーンの link 検査、ADR-0027) ---

const CHAIN_2: &str = r#"[{"number":38,"headRefName":"stage1","baseRefName":"main"},{"number":39,"headRefName":"stage2","baseRefName":"stage1"}]"#;
const CHAIN_1: &str = r#"[{"number":38,"headRefName":"stage1","baseRefName":"main"}]"#;
const STACKS_LINKED: &str = r#"[{"open":true,"pull_requests":[{"number":38},{"number":39}]}]"#;

/// 旧 selftest の `gstack <sid> <branch> <pr_num> <base> <chain> [<stacks>] [<stacks_rc>] [<ext_rc>]`。
#[allow(clippy::too_many_arguments)]
fn gstack(
    fx: &Fx,
    sid: &str,
    br: &str,
    num: &str,
    base: &str,
    chain: &str,
    stacks: Option<&str>,
    stacks_rc: &str,
    ext_rc: &str,
) -> common::Run {
    fx.git(&["switch", "-q", br]);
    let chainf = fx.write(&format!("{sid}-chain.json"), chain);
    let stacksf = stacks
        .map(|s| fx.write(&format!("{sid}-stacks.json"), s))
        .unwrap_or_default();
    ci(
        fx,
        sid,
        Some("[]"),
        CHECKS_QUIESCE_PASS,
        &[
            ("PR_GATE_STUB_PR_NUM", num),
            ("PR_GATE_STUB_BASE", base),
            ("PR_GATE_STUB_CHAIN_FILE", &chainf),
            ("PR_GATE_STUB_STACKS_FILE", &stacksf),
            ("PR_GATE_STUB_STACKS_RC", stacks_rc),
            ("PR_GATE_STUB_STACK_EXT_RC", ext_rc),
        ],
    )
}

fn stack_fixture() -> Fx {
    let fx = Fx::new();
    fx.git(&["branch", "-q", "stage1"]);
    fx.git(&["branch", "-q", "stage2"]);
    fx
}

#[test]
fn g_stack_unlinked_chain_blocks() {
    let fx = stack_fixture();
    let r = gstack(
        &fx,
        "chain-block-sid",
        "stage2",
        "39",
        "stage1",
        CHAIN_2,
        Some("[]"),
        "0",
        "0",
    );
    assert_eq!(r.code, 2); // ok   チェーンあり・未リンクは block(exit 2)
    assert!(has(&r.stderr, "gh stack link 38 39")); // ok   gh stack link コマンドを案内(PR番号、bottom→top)
}

#[test]
fn g_stack_linked_chain_passes() {
    let fx = stack_fixture();
    let r = gstack(
        &fx,
        "chain-linked-sid",
        "stage2",
        "39",
        "stage1",
        CHAIN_2,
        Some(STACKS_LINKED),
        "0",
        "0",
    );
    assert_eq!(r.code, 0); // ok   リンク済みは pass(exit 0)
}

#[test]
fn g_stack_single_pr_is_silent() {
    let fx = stack_fixture();
    let r = gstack(
        &fx,
        "chain-single-sid",
        "stage1",
        "38",
        "main",
        CHAIN_1,
        Some("[]"),
        "0",
        "0",
    );
    assert_eq!(r.code, 0); // ok   単独 PR(chain size 1)は沈黙・pass(exit 0)
}

#[test]
fn g_stack_without_extension_is_advisory() {
    let fx = stack_fixture();
    let r = gstack(
        &fx,
        "chain-noext-sid",
        "stage2",
        "39",
        "stage1",
        CHAIN_2,
        None,
        "0",
        "1",
    );
    assert_eq!(r.code, 0); // ok   gh-stack 拡張不在は advisory 降格・pass(exit 0)
    assert!(has(&r.stderr, "gh-stack 拡張が無い")); // ok   拡張不在の advisory 文言
}

#[test]
fn g_stack_api_failure_is_advisory() {
    let fx = stack_fixture();
    let r = gstack(
        &fx,
        "chain-apifail-sid",
        "stage2",
        "39",
        "stage1",
        CHAIN_2,
        Some("[]"),
        "1",
        "0",
    );
    assert_eq!(r.code, 0); // ok   stacks API 失敗は advisory 降格・pass(exit 0)
    assert!(has(&r.stderr, "stacks API の取得に失敗")); // ok   stacks API 失敗の advisory 文言
}

// --- escalate (独自カウンタ + 上限) ---

#[test]
fn escalates_after_max_blocks_then_passes() {
    let fx = Fx::new();
    let mut last = None;
    for _ in 0..6 {
        last = Some(fx.stop("esc-sid", &[]));
    }
    let last = last.unwrap();
    // ok   6 回目で escalate 文言
    assert_eq!(
        last.stderr
            .lines()
            .filter(|l| l.contains("AskUserQuestion"))
            .count(),
        1
    );
    let r = fx.stop("esc-sid", &[]);
    assert_eq!(r.code, 0); // ok   escalated 後は素通り(exit 0)
    assert_eq!(r.all(), ""); // ok   escalated 後は出力なし
}

// --- G_prior(ADR-543)— 移植時に追加(旧 selftest は new-tool-guard バイナリの
// 有無に依存するため検査していなかった。Rust 版は判定関数を直接リンクする) ---

#[test]
fn g_prior_blocks_new_tool_without_kizon() {
    let fx = Fx::new();
    std::fs::create_dir_all(fx.repo.join("scripts")).unwrap();
    std::fs::write(fx.repo.join("scripts/a"), "#!/bin/sh\n").unwrap();
    std::fs::write(fx.repo.join("scripts/b"), "x\n").unwrap();
    fx.git(&["add", "scripts"]);
    fx.commit("tools");
    let head = fx.rev("HEAD");
    let checks = fx.write("c.json", CHECKS_QUIESCE_PASS);
    let base_env = [
        ("PR_GATE_STUB_HEAD_OID", head.as_str()),
        ("PR_GATE_STUB_CHECKS_FILE", checks.as_str()),
    ];
    if std::env::var_os("PR_GATE_ORACLE").is_some() {
        // bash 版は new-tool-guard バイナリ(NEW_TOOL_GUARD_BIN)を要するが、
        // このクレートのテストからは参照できないのでオラクル実行では省く。
        return;
    }
    let r = fx.stop("prior-sid", &base_env);
    assert_eq!(r.code, 2, "{}", r.stderr);
    // bash 版の printf の都合で 2 件目以降には "  - " が付かない(挙動を保存)。
    assert!(
        r.stderr.contains("  - scripts/a\nscripts/b\n"),
        "{}",
        r.stderr
    );

    let mut env = base_env.to_vec();
    let body =
        "Closes #1\nNo-Visual: x\n既存手段: scripts/a — 採用: sh\n既存手段: scripts/b — 採用: sh";
    env.push(("PR_GATE_STUB_PR_BODY", body));
    let r = fx.stop("prior-ok-sid", &env);
    assert_eq!(r.code, 0, "{}", r.stderr);
}
