# Self-hosted LiveSync の公式 CLI(上流 `src/apps/cli`)を、タグ 1.0.44-cli のソースから
# ビルドする(#1328)。nixpkgs に該当するパッケージは無く、release asset にも CLI の
# bundle が無い(plugin の 3 点だけ)ので、`buildNpmPackage` で build する。
#
# 上流の vite 設定は pouchdb-*・leveldown・chokidar・werift を `dist/index.cjs` に
# 同梱せず外に出す。そのため出力には `dist/` と、実行時の依存だけの `node_modules` の
# 両方を置く(`node_modules` は `dist/` の 1 つ上に置き、Node の解決で見つかる)。
# leveldown は `prebuilds/` を持つ node-gyp-build 型で、`installCheckPhase` が実際に読み込む。
{
  lib,
  buildNpmPackage,
  fetchFromGitHub,
  makeWrapper,
  nodejs_22,
  python3,
  pkg-config,
}:
buildNpmPackage {
  pname = "obsidian-livesync-cli";
  version = "1.0.44";

  src = fetchFromGitHub {
    owner = "vrtmrz";
    repo = "obsidian-livesync";
    # タグ 1.0.44-cli の commit(`gh api repos/vrtmrz/obsidian-livesync/git/ref/tags/1.0.44-cli`、
    # 取得 2026-11-05)。タグは動かせるので、commit で固定する。
    rev = "72d9e22bc351043ffc8790425b5bac897c80eb37";
    hash = "sha256-q3dAJXRHokGSrczq3JCChPfmr89g7ybmd+/4DOXpMcE=";
  };

  nodejs = nodejs_22;
  npmDepsHash = "sha256-qDRpoj4+U8E1J1DuFd6fOpvsgEHQJtJBpyRlyr9utnQ=";
  npmWorkspace = "src/apps/cli";

  nativeBuildInputs = [
    makeWrapper
    python3
    pkg-config
  ];

  # 既定の install(`npm pack` した 1 つの workspace)は、`dist/` と実行時の依存を
  # 別々に置けない。root の `node_modules` から開発用の依存を落とし、workspace 自身への
  # symlink(`self-hosted-livesync-cli` など。出力の外を指して noBrokenSymlinks で落ちる)を
  # 取り除いて、`libexec` に置く。
  dontNpmInstall = true;
  installPhase = ''
    runHook preInstall

    npm prune --omit=dev --offline --no-audit --no-fund
    find node_modules -maxdepth 1 -type l -delete

    libexec=$out/libexec/obsidian-livesync-cli
    mkdir -p "$libexec" $out/bin
    cp -r src/apps/cli/dist "$libexec/dist"
    cp -r node_modules "$libexec/node_modules"
    makeWrapper ${lib.getExe nodejs_22} $out/bin/obsidian-livesync-cli \
      --add-flags "$libexec/dist/index.cjs"

    runHook postInstall
  '';

  doInstallCheck = true;
  installCheckPhase = ''
    runHook preInstallCheck

    export HOME="$TMPDIR/home"
    mkdir -p "$HOME"
    $out/bin/obsidian-livesync-cli --help > help.txt
    grep -q daemon help.txt

    # 外に出された依存(native の leveldown を含む)が、出力の中から解決できること。
    ${lib.getExe nodejs_22} -e '
      const { createRequire } = require("node:module");
      const r = createRequire(process.argv[1] + "/libexec/obsidian-livesync-cli/dist/index.cjs");
      for (const m of ["leveldown", "pouchdb-adapter-leveldb", "pouchdb-adapter-http", "chokidar", "werift"]) {
        r(m);
      }
    ' "$out"

    runHook postInstallCheck
  '';

  meta = {
    description = "Self-hosted LiveSync の公式 CLI(Obsidian を起動せずに vault を同期する)";
    homepage = "https://github.com/vrtmrz/obsidian-livesync";
    license = lib.licenses.mit;
    mainProgram = "obsidian-livesync-cli";
    platforms = lib.platforms.linux;
  };
}
