# ja-polish が呼ぶ `llm`(Simon Willison の LLM CLI)と、Gemini 用の `llm-gemini` プラグイン。
#
# 固定中の nixpkgs(stable)は llm 0.30・llm-gemini 0.31 で、unstable も llm-gemini 0.32
# 止まり。無料枠で使える最新の `gemini-3.8-flash` に llm-gemini が対応したのは 0.34 で、
# 0.35 はそれに加えて `llm>=0.32` を要求する(上流の pyproject.toml、取得 2026-11-08)。
# llm は 0.32 で足りる: 最新の 0.37.1 は `openai>3` を要求するが、nixpkgs は 2.33.0 で、
# 0.32 の `openai>=2.32.0` は満たす。一方 llm 0.32 は、nixpkgs の版では満たせない次の 2 つ
# を要求する(nixpkgs の版は 2026-11-08 に `nix eval` で確認)ので、あわせて上げる:
#   - sqlite-utils >= 4.0(nixpkgs は 3.39)
#   - condense-json >= 1.1(nixpkgs は 0.1.3)
# `httpx`・`ijson`・`openai` などは nixpkgs の版のまま。
#
# 4 つとも nixpkgs が追いついたら、この overlay ごと外して `llm.withPlugins` にする
# (herdr の overlay と同じ扱い。docs/agents/ja-polish.md)。
#
# 上流のタグは動かせるので、commit で固定する(obsidian-livesync-cli.nix と同じ)。
# テストは走らせない: llm のテストは `llm-echo` を、llm-gemini のテストは録画した通信を
# 前提にしていて、ビルドの sandbox の中では再現できない。代わりに `pythonImportsCheck` と
# `pythonRuntimeDepsCheck`(要求する版が満たされているか)が走り、実際の呼び出しは
# docs/agents/ja-polish.md の「実測」で確かめる。nixpkgs の `llm install` / `uninstall` を
# 無効にするパッチは 0.32 に当たらないので外す(プラグインを pip で入れない環境なので実害は無い)。
{ python3, fetchFromGitHub }:
let
  py = python3.override {
    packageOverrides = final: prev: {
      condense-json = prev.condense-json.overridePythonAttrs (_old: {
        version = "1.1";
        src = fetchFromGitHub {
          owner = "simonw";
          repo = "condense-json";
          rev = "27e812f0765c396eb9f21c7e5172667290c0a9c0"; # 1.1
          hash = "sha256-IBYjDFhbQlZ/17nTo5FvJM7aeadKS5dW7J8IGy4956M=";
        };
        doCheck = false;
      });

      sqlite-utils = prev.sqlite-utils.overridePythonAttrs (_old: {
        version = "4.2.1";
        src = fetchFromGitHub {
          owner = "simonw";
          repo = "sqlite-utils";
          rev = "28dc6278cc03a9245325d056e6986818544abc68"; # 4.2.1
          hash = "sha256-PuAQ1dk999St3Wt4tGBgFUuPehv7ZTf4T7dAC+r1Pis=";
        };
        doCheck = false;
      });

      llm = prev.llm.overridePythonAttrs (_old: {
        version = "0.32";
        src = fetchFromGitHub {
          owner = "simonw";
          repo = "llm";
          rev = "71a4815a2cd1957febcac5f566fb13a483fc30f5"; # 0.32
          hash = "sha256-lDPF4Z+U9Zlqc1Dt7pCrxmthAZj4a0hNpz5d8J7TtM8=";
        };
        patches = [ ];
        postPatch = "";
        # 上流の pyproject.toml の [project].dependencies(0.32)。0.30 から numpy・
        # sqlite-migrate が減った。
        dependencies = with final; [
          click
          condense-json
          openai
          click-default-group
          sqlite-utils
          pydantic
          pyyaml
          pluggy
          python-ulid
          setuptools
          pip
          puremagic
        ];
        doCheck = false;
      });

      llm-gemini = prev.llm-gemini.overridePythonAttrs (_old: {
        version = "0.35";
        src = fetchFromGitHub {
          owner = "simonw";
          repo = "llm-gemini";
          rev = "337e3f6c4795cdbffdb7249959c87ae1281bdef2"; # 0.35
          hash = "sha256-Yi5Z6qyiDTGmCLa+H0R+u2mcUW+PbmFW9vxOhUEJqTY=";
        };
        doCheck = false;
      });
    };
    self = py;
  };
in
py.pkgs.llm.withPlugins { llm-gemini = true; }
