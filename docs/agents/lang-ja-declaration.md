# lang-ja-declaration — 日本語が正本のリポジトリを、宣言 1 つで CJK 検査から外す

設計判断の記録: `docs/adr/805-lang-ja-declaration.md`
宣言: リポジトリのトピック `lang-ja`
読む側: `crates/github-audit`(charters ドメイン)
付ける側: `config/agents/skills/repo-charter/SKILL.md`(charter インタビュー)

## 何のための宣言か

README と CONTRIBUTING は英語の原文 1 本(ADR-0016 Decision 4)で、CJK が 5 文字以上あるだけで
`readme-cjk-present` / `contributing-cjk-present` の drift になる(ADR-0017 Decision 3)。読者が日本語話者に
限られるリポジトリでは、この前提が成り立たない。そこで、正本言語が日本語であることを宣言し、CJK の検査
だけを外す。

## 宣言があると何が変わるか

| 検査 | 宣言ありのとき |
|---|---|
| `readme-cjk-present` / `contributing-cjk-present` | 出さない |
| 見出しのリテラル、節の順序、目的文と description の一致、nav-doc、ルート allowlist | 変わらない(英語の見出しのまま) |
| `lang-ja-stale` | README の CJK が閾値(5 文字)未満なら出す |

## 宣言が嘘になったことを検出する(`lang-ja-stale`)

宣言があるのに README に日本語がほとんど無いとき、charters ドメインが報告する。英語に書き直したら
トピックを外す。

## 宣言の付け方

```bash
gh repo edit <owner>/<repo> --add-topic lang-ja
```

外すとき:

```bash
gh repo edit <owner>/<repo> --remove-topic lang-ja
```

## 確かめ方

```bash
GITHUB_AUDIT_OWNER=<owner> github-audit --json charters | jq '.[] | select(.repo=="<repo>")'
```

`missing` に `readme-cjk-present` と `contributing-cjk-present` が出ず、`lang-ja-stale` も出ないこと。
