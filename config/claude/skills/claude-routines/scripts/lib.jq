# lib.jq — claude-routines の宣言(.claude/routines/*.json + <name>.md)と
# live trigger(RemoteTrigger get/list が返す生 JSON)を同じ形の正規化
# 射影(projection)に変換する共有関数。ハッシュ化(sha256)自体はここでは
# 行わない — jq に組み込みが無いため、routines-plan.sh 側が coreutils の
# sha256sum に委ねる(このファイルは正規化 JSON 文字列を作るところまで)。
#
# 設計と根拠: docs/claude/claude-routines.md、
# docs/adr/519-routines-declaration-in-repo.md。

# "https://github.com/owner/repo" / "git@github.com:owner/repo.git" のような
# git URL から "owner/repo" を取り出す。
def repo_from_git_url:
  capture("github\\.com[:/](?<repo>[^/]+/[^/]+?)(\\.git)?$"; "i").repo;

# 宣言 JSON(1 routine 分、`.` に入っている想定)+ prompt md 本文
# (routine-spec 行を含まない生テキスト)から正規化射影を作る。
# `connectors`/`mcp_connections` は射影に含めない — 全 routine で共通の
# アカウント既定セットであり、routine ごとの宣言対象ではない(2026-09-27
# 実測、段0a)。
def decl_projection($md_body):
  {
    name: (.home_repo + ":" + .name),
    cron_utc: .cron_utc,
    model: .model,
    environment_id: .environment_id,
    sources: ((.sources // []) | sort),
    allowed_tools: ((.allowed_tools // []) | sort),
    enabled: (.state == "enabled"),
    prompt_body: ($md_body | rtrimstr("\n"))
  };

# live trigger の prompt 本文そのもの(routine-spec 注記行を含む)。
def live_prompt_content:
  .job_config.ccr.events[0].data.message.content // "";

# live prompt の末尾の `routine-spec: <sha256 hex>` 行から hash 値だけを
# 取り出す。無ければ null(捕捉なしなら capture は空ストリームを返すため
# `//` で null に倒す)。
def live_annotation:
  live_prompt_content
  | (capture("routine-spec: (?<h>[0-9a-f]{64})\\s*$").h) // null;

# live prompt から routine-spec 注記行(直前の空行を含む)を取り除いた本文。
def live_prompt_body:
  live_prompt_content
  | if test("\\n+routine-spec: [0-9a-f]{64}\\s*$")
    then sub("\\n+routine-spec: [0-9a-f]{64}\\s*$"; "")
    else .
    end;

# live trigger(`.` に RemoteTrigger get/list の1件分が入っている想定)から
# decl_projection と同じ形の正規化射影を作る。
def live_projection:
  {
    name: .name,
    cron_utc: (.cron_expression // ""),
    model: (.job_config.ccr.session_context.model // ""),
    environment_id: (.job_config.ccr.environment_id // ""),
    sources: ((.job_config.ccr.session_context.sources // [])
      | map(.git_repository.url | repo_from_git_url) | sort),
    allowed_tools: ((.job_config.ccr.session_context.allowed_tools // []) | sort),
    enabled: .enabled,
    prompt_body: live_prompt_body
  };

# 宣言(`.` に decl JSON)から RemoteTrigger create/update にそのまま渡す
# body を組み立てる。$hash は routines-plan.sh が decl_projection の
# 正規化 JSON から sha256sum で計算した hex 文字列。段0b(2026-09-27)で
# 実測した「job_config.ccr に触れる更新は全置換」という挙動に合わせ、
# 常に session_context を完全な形で送る。
def build_body($md_body; $hash):
  {
    name: (.home_repo + ":" + .name),
    cron_expression: .cron_utc,
    enabled: (.state == "enabled"),
    job_config: {
      ccr: {
        environment_id: .environment_id,
        events: [{
          data: {
            type: "user",
            message: {
              role: "user",
              content: (($md_body | rtrimstr("\n")) + "\n\nroutine-spec: " + $hash)
            }
          }
        }],
        session_context: {
          model: .model,
          allowed_tools: ((.allowed_tools // []) | sort),
          sources: ((.sources // []) | sort
            | map({git_repository: {url: ("https://github.com/" + .)}}))
        }
      }
    }
  };

# live trigger が cron routine か(run-once/webhook は cron_expression
# を持たない)。
def is_cron:
  ((.cron_expression // "") | length) > 0;
