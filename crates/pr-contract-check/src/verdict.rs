//! 3 つの検査が共有する、判定の結果の表現と出力。

/// 判定の結果。`out` は標準出力へ、`err` は標準エラーへ、順に出す。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Verdict {
    pub code: i32,
    pub out: Vec<String>,
    pub err: Vec<String>,
}

impl Verdict {
    /// 取得に失敗したときの結果(fail-closed: 契約の前提が確認できないまま通さない)。
    pub fn fail_closed(msg: impl Into<String>) -> Verdict {
        Verdict {
            code: 1,
            out: vec![],
            err: vec![msg.into()],
        }
    }

    /// 出力して、終了コードを返す。
    pub fn emit(&self) -> i32 {
        for l in &self.out {
            println!("{l}");
        }
        for l in &self.err {
            eprintln!("{l}");
        }
        self.code
    }
}
