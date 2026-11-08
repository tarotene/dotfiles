//! calendar-handshake-reminder の純関数部分(SessionStart hook、docs/agents/calendar-handshake.md)。
//!
//! 勤務先の機体で、握手カレンダーに下書き待ちの印(`blocked`)の予定があるかを確かめる
//! 手順を `additionalContext` で注入する。起動時の hook からはカレンダーのコネクタを
//! 読めないので、件数は数えない。読み取りも下書きの作成もエージェントが行い、送信は
//! 本人がする(外部発信は下書き止まり)。印のトークンは語彙 JSON(正本)から取る。
//! I/O は main.rs だけが行う。

use serde_json::Value;

/// 語彙の正本(`config/agents/calendar-handshake/vocabulary.json`)。
const VOCABULARY_JSON: &str =
    include_str!("../../../config/agents/calendar-handshake/vocabulary.json");

/// 語彙 JSON から、状態 `id` の印(末尾の空白まで含めたトークン)を取る。
pub fn token_of(vocabulary: &str, id: &str) -> Result<String, String> {
    let v: Value =
        serde_json::from_str(vocabulary).map_err(|e| format!("語彙が JSON でない: {e}"))?;
    v["states"]
        .as_array()
        .ok_or("語彙に states が無い")?
        .iter()
        .find(|s| s["id"] == id)
        .and_then(|s| s["token"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("語彙に状態 {id} の印が無い"))
}

/// 正本の語彙から、`(blocked の印, secured の印)`。
pub fn tokens() -> (String, String) {
    let get = |id| token_of(VOCABULARY_JSON, id).expect("calendar-handshake の語彙が壊れている");
    (get("blocked"), get("secured"))
}

/// 注入文。読み取りと下書きまでを手順にし、送信は本人に残す。
pub fn build_context(blocked: &str, secured: &str) -> String {
    format!(
        "この機体は勤務先です。個人側と勤務先側のカレンダーの握手(`docs/agents/calendar-handshake.md`)で、\n\
         下書き待ちの印 `{blocked}` の予定がたまっていないかを確かめてください。件数はここでは数えていません。\n\
         読み取りはあなたが行います(カレンダーのコネクタで、握手の専用カレンダーの題名が `{blocked}` で\n\
         始まる予定を探す)。読めない、または 0 件なら、本人に何も報告せず、このセッションの依頼を進めてください。\n\
         \n\
         1 件以上あれば、本人に件数を一言で伝え、予定ごとに次の下書きを作ります。\n\
         - 休みの連絡: 関係者への文面。\n\
         - リスケの依頼: 休みと重なる会議の主催者への文面。\n\
         - 半休の申請: 勤怠に出す内容(日付・時間帯・理由)。\n\
         下書きは Slack・メールの下書きまでで止め、送信はしません。送信と申請の確定は本人がします。\n\
         \n\
         本人が送った(申請した)ことを、送信済みの状態などを読み戻して確かめたら、その予定の題名の印を\n\
         `{blocked}` から `{secured}` に直します。確かめる手段が無い申請(勤怠システムなど)は、本人に\n\
         済んだかを聞き、本人の返事を「本人申告」と明記して記録してから印を直します。\n\
         確かめられていないものを `{secured}` にしません。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_come_from_the_vocabulary() {
        let (blocked, secured) = tokens();
        assert_eq!(blocked, "[blocked] ");
        assert_eq!(secured, "[secured] ");
    }

    #[test]
    fn token_of_reads_one_state_or_says_why_not() {
        let v = r#"{"states":[{"id":"a","token":"[a] "}]}"#;
        assert_eq!(token_of(v, "a"), Ok("[a] ".into()));
        assert!(token_of(v, "b").unwrap_err().contains("b"));
        assert!(token_of("{}", "a").is_err());
        assert!(token_of("not json", "a").is_err());
    }

    #[test]
    fn context_gives_the_procedure_and_leaves_sending_to_the_person() {
        let c = build_context("[blocked] ", "[secured] ");
        assert!(c.contains("`[blocked] `"));
        assert!(c.contains("`[secured] `"));
        for kind in ["休みの連絡", "リスケの依頼", "半休の申請"] {
            assert!(c.contains(kind), "{kind}");
        }
        assert!(c.contains("送信はしません"));
        assert!(c.contains("本人申告"));
        assert!(c.contains("件数はここでは数えていません"));
    }
}
