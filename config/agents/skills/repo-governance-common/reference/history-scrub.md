# 署名必須のリポジトリで、混入データを履歴から消す

`required_signatures` の ruleset を持つリポジトリで、誤ってコミットしたデータ(秘密、生成物、
個人情報、ブラウザのプロファイルなど)を、main の履歴から消す手順。署名を要求するリポジトリでは、
履歴の書き換えが全コミットの再署名に波及する点が、署名の無いリポジトリと違う。

## 0. 先にやること

- **秘密なら、先に失効・再発行する。** GitHub の手順は「最初の手順として、その秘密を失効または
  ローテーションする」と書く(GitHub Docs, "Removing sensitive data from a repository"、
  取得 2026-10-21)。履歴から消しても、他の人の clone と、すでに読まれた可能性は消せない
  (同ページ: 「他の利用者の clone からは消せない」)。
- **混入の範囲を測る。** 混入したコミットがどれか、そこから後ろに何コミットあるか、混入を含む
  ブランチと、混入を参照する PR(`refs/pull/*`)がいくつあるかを数える。
- **ruleset の `bypass_actors` は足さない。** 一時的に迂回する用の主体を残すと、元に戻したあとも
  迂回の穴が残る。次の 3 節のとおり、`enforcement` の切り替えだけで済ませる。

## 1. 手順の選び方

| 状況 | 手順 |
|---|---|
| 混入が最近で、後続が少なく、分かれたブランチが少ない | **作り直して載せ替える**(2 節) |
| 混入が古く、広範囲、または複数のパスにまたがる | GitHub の公式手順 `git filter-repo --sensitive-data-removal`(3 節) |

GitHub の公式手順は、`git-filter-repo`(2.47 以上)を `--sensitive-data-removal` と
`--invert-paths --path <混入したパス>`(または `--replace-text`)で実行する(同ページ)。
ただし **`git filter-repo` は内部で fast-export を使い、コミットの署名を剥がす**。署名必須の
リポジトリでは、混入より前のコミットを含む全コミットとリリースタグが書き換わり、
署名も失われる(実施記録にもとづく。使い捨てのミラーで実行して、全コミットが書き換わることを
確かめた。一次文書では未確認)。まず使い捨てのミラー(`git clone --mirror`)で試し、書き換わる
コミット数を数えてから選ぶ。

## 2. 作り直して載せ替える(署名を保つ)

署名鍵がスマートカードの外にある署名用サブ鍵なら、再署名にタッチは要らない。

1. 混入コミットを `git commit --amend`(混入したパスを外す)で作り直す。
2. 後続を `git rebase --committer-date-is-author-date --onto <新しい混入コミット> <旧い混入コミット> <ブランチ>`
   で載せ替える。`commit.gpgsign=true` なら、載せ替えたコミットは再署名される。
   `--committer-date-is-author-date` は、日付が一斉に今日になるのを避ける。
3. 混入を含む**他のブランチ**も、旧い main との分岐点を、旧コミットと新コミットの対応表で
   新しい main に付け替える。旧い履歴を持つローカルブランチを、載せ替えずに push すると、
   混入が復活する。
4. 載せ替えた結果で、混入したパスが main のどのコミットにも無いことを読み戻す
   (`git log --all --oneline -- <パス>` が空)。

## 3. main に push する(ruleset を一時的に止める)

main に効く ruleset の `non_fast_forward` や `required_signatures` は、書き換えた履歴の push を
拒否する。`gh api -X PUT repos/OWNER/REPO/rulesets/<id>` で `enforcement` を `disabled` にして
push し、**直後に** `active` へ戻して、読み戻す(`gh api repos/OWNER/REPO/rulesets/<id>`)。
止めている間に他の push が通るので、短く終える。`--force-with-lease --force-if-includes` で押す。

## 4. 後始末

- **プレビュー用のタグ・prerelease**: 混入を含むものを削除し、CI の再生成に任せる。
- **`refs/pull/*` は push で消せない。** GitHub が読み取り専用にしているため。GitHub Support
  に、リポジトリ名、影響を受ける PR の数、`git filter-repo` が報告する最初に変わったコミット
  (作り直しなら、混入コミットの旧い SHA と、書き換え前の head)を添えて依頼する。Support が
  PR の参照の外し、キャッシュされた表示の削除、ガベージコレクションを行う(同ページ)。
- **他の利用者**: 各自の clone では、merge でなく rebase で追従してもらう(同ページ)。
  作業ツリーの古いブランチを使い回さない。

## 実施記録を残す

誰が、いつ、どの範囲(コミット数、ブランチ、タグ、PR 数)を、どの手順で消したかを、
リポジトリの Issue か PR に残す。混入を含んでいた旧い SHA は、Support への依頼で要るので控える。
