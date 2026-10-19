# docs-fixtures

`.github/actions/docs-{rust,python,typescript}` の回帰用 fixture。正常な状態で
成功し、CI の `docs-actions` job が壊したコピーで失敗することを検証する。

`python-myst` は pyproject.toml に `docs` extra を持つ Python パッケージの
fixture。`docs/conf.py` が `myst_parser` を使うので、action が `.[docs]` を
入れなければ Markdown のページを読めずに落ちる。
