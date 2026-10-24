//! `gh api` の呼び出し。JSON は読まず、`--jq` で射影した出力(行)だけを読む。
//!
//! 各ツールは `gh` の実行ファイルを環境変数で差し替えられる(テストのスタブ用)。
//! 既定は PATH 上の `gh`。

use std::io::Write;
use std::process::{Command, Stdio};

/// 1 回の呼び出しの結果。起動できなかったときも `ok = false`(`stderr` に理由)。
#[derive(Debug, Clone, Default)]
pub struct Out {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct Gh {
    bin: String,
}

impl Gh {
    pub fn new(bin: impl Into<String>) -> Gh {
        Gh { bin: bin.into() }
    }

    /// 環境変数 `var` が非空ならその値、無ければ `gh`。
    pub fn from_env(var: &str) -> Gh {
        match std::env::var(var) {
            Ok(v) if !v.is_empty() => Gh::new(v),
            _ => Gh::new("gh"),
        }
    }

    /// `gh api <path> <args>…`。stdin は閉じる。
    pub fn api(&self, path: &str, args: &[&str]) -> Out {
        match Command::new(&self.bin)
            .arg("api")
            .arg(path)
            .args(args)
            .stdin(Stdio::null())
            .output()
        {
            Ok(o) => Out {
                ok: o.status.success(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            },
            Err(e) => Out {
                ok: false,
                stdout: String::new(),
                stderr: format!("{}: {e}", self.bin),
            },
        }
    }

    /// `gh api <path> <args>… --jq <expr>` の出力を行に分ける(末尾の改行は落とす)。
    /// 失敗は `None`。成功して出力が空なら空の `Vec`。
    pub fn lines(&self, path: &str, args: &[&str], jq: &str) -> Option<Vec<String>> {
        let mut a: Vec<&str> = args.to_vec();
        a.extend(["--jq", jq]);
        let out = self.api(path, &a);
        out.ok.then(|| split_lines(&out.stdout))
    }

    /// `repos/<repo>/contents/<path>` の、`git_ref` 時点のファイルの中身(raw)。
    /// ファイルが無い・取れないときは `ok = false`。
    pub fn raw_file(&self, repo: &str, path: &str, git_ref: &str) -> Out {
        let ref_arg = format!("ref={git_ref}");
        self.api(
            &format!("repos/{repo}/contents/{path}"),
            &[
                "-X",
                "GET",
                "-F",
                &ref_arg,
                "-H",
                "Accept: application/vnd.github.raw+json",
            ],
        )
    }
}

pub fn split_lines(s: &str) -> Vec<String> {
    s.lines().map(str::to_owned).collect()
}

/// `jq <args>` に `input` を標準入力で渡し、成功したときだけ標準出力を返す。
/// JSON を自前で読まない代わりに、CI のランナーにも devShell にもある `jq` に任せる。
pub fn jq(args: &[&str], input: &str) -> Option<String> {
    let mut child = Command::new("jq")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    // 書き込みが終わる前に jq が出力で詰まらないよう、別スレッドで書く。
    let data = input.to_owned();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(data.as_bytes());
    });
    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
