# GitHub の持ち主(owner)から側(personal | company)への表の宣言口。
#
# 機体の Claude の契約の側(個人 / 会社)で、読んでよいリポジトリが決まる(`repo-scope-guard`、
# docs/agents/repo-scope-guard.md)。個人の契約の機体は、別の側・表にない持ち主の**非公開**リポジトリを
# 読まない。その「どの持ち主がどちらの側か」の表を、ここが `~/.config/dotfiles/github-owners.toml` として配る。
#
# 規則とスキーマはこの公開リポジトリが持ち、**値(実在の持ち主の名前)は書かない**。値は、非公開の
# 設定リポジトリが `dotfiles.github.owners` に入れる(ADR-0034)。既定は空で、空のときはファイルを
# 配らない = この機体にこの規則を配っていない(何も制限しない)。順序に注意: このオプションを使う
# 非公開側が、この宣言を取り込むより前に値を入れると、評価が失敗する。
#
# 表の形は、`[owners]` に `<持ち主> = "personal" | "company"` を並べる TOML。キーは大文字小文字を区別しない
# (GitHub のログイン名と同じ)ので、大文字小文字だけが違う同じ持ち主を 2 回入れると評価が失敗する。
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.dotfiles.github.owners;
  names = lib.attrNames cfg;
in
{
  options.dotfiles.github.owners = lib.mkOption {
    type = lib.types.attrsOf (
      lib.types.enum [
        "personal"
        "company"
      ]
    );
    default = { };
    description = ''
      GitHub の持ち主(ユーザー・組織のログイン名)から、側(`personal` / `company`)への表。
      閉じた 2 語で、これ以外の値は表せない。表にない持ち主は、個人の契約の機体では「別の側」と同じに扱われる。
    '';
  };

  config = lib.mkIf (cfg != { }) {
    assertions = [
      {
        assertion = lib.length (lib.unique (map lib.toLower names)) == lib.length names;
        message = "dotfiles.github.owners: 大文字小文字だけが違う同じ持ち主が 2 回ある(GitHub のログイン名は大文字小文字を区別しない)。";
      }
    ];

    xdg.configFile."dotfiles/github-owners.toml".source =
      (pkgs.formats.toml { }).generate "github-owners.toml"
        { owners = cfg; };
  };
}
