#!/usr/bin/env zsh
# nixgl-restore.test.zsh — config/zsh/modules/05-nixgl-restore.zsh の挙動試験(#851)。
#
# nixGL の wrapper(home/modules/nixgl.nix)は、GL の変数を足す前の値を
# `__NIXGL_SAVED_<VAR>` に退避し(元が未設定なら `__NIXGL_UNSET__`)、`NIXGL_WRAPPED=1` を
# 立てる。モジュールは、印があるときだけ、対話シェルで元の値へ戻す。
#   zsh config/zsh/tests/nixgl-restore.test.zsh
# CI の zsh の step(.github/workflows/ci.yml)が実行する。

emulate -L zsh
mod=${0:A:h}/../modules/05-nixgl-restore.zsh
fail=0
vars=(GBM_BACKENDS_PATH LIBGL_DRIVERS_PATH LIBVA_DRIVERS_PATH __EGL_VENDOR_LIBRARY_FILENAMES LD_LIBRARY_PATH)

# 環境を空にして、与えた変数だけでモジュールを読み込み、`name=value`(未設定は `name=<unset>`)を返す。
run_case() { # $1=読み込み後に調べる変数の空白区切り  $2...=env に渡す NAME=VALUE
    local probe=$1; shift
    env -i HOME="$HOME" PATH="$PATH" "$@" zsh -f -c '
        source '"${(q)mod}"'
        for n in '"$probe"'; do
            if (( ${+parameters[$n]} )); then print -r -- "$n=${(P)n}"; else print -r -- "$n=<unset>"; fi
        done
        print -r -- "env=$(env | grep -c "^__NIXGL_SAVED_\|^NIXGL_WRAPPED=")"
    '
}

expect() { # $1=名前  $2=期待  $3=実際
    if [[ $2 == $3 ]]; then
        print -r -- "ok   $1"
    else
        print -r -- "FAIL $1"; print -r -- "  want: ${2//$'\n'/ | }"; print -r -- "  got:  ${3//$'\n'/ | }"
        fail=1
    fi
}

all_vars="${vars[*]}"

# 1. 印があれば、退避した元の値に戻し、印と退避を消す(子に残さない)。
got=$(run_case "$all_vars NIXGL_WRAPPED __NIXGL_SAVED_LD_LIBRARY_PATH" \
    NIXGL_WRAPPED=1 \
    LD_LIBRARY_PATH=/nix/mesa/lib:/orig/lib __NIXGL_SAVED_LD_LIBRARY_PATH=/orig/lib \
    LIBGL_DRIVERS_PATH=/nix/mesa/dri __NIXGL_SAVED_LIBGL_DRIVERS_PATH=__NIXGL_UNSET__ \
    GBM_BACKENDS_PATH=/nix/mesa/gbm __NIXGL_SAVED_GBM_BACKENDS_PATH=__NIXGL_UNSET__ \
    LIBVA_DRIVERS_PATH=/nix/mesa/dri:/nix/va __NIXGL_SAVED_LIBVA_DRIVERS_PATH=__NIXGL_UNSET__ \
    __EGL_VENDOR_LIBRARY_FILENAMES=/nix/50_mesa.json:/orig/vendor.json __NIXGL_SAVED___EGL_VENDOR_LIBRARY_FILENAMES=/orig/vendor.json)
expect "1 restore" "GBM_BACKENDS_PATH=<unset>
LIBGL_DRIVERS_PATH=<unset>
LIBVA_DRIVERS_PATH=<unset>
__EGL_VENDOR_LIBRARY_FILENAMES=/orig/vendor.json
LD_LIBRARY_PATH=/orig/lib
NIXGL_WRAPPED=<unset>
__NIXGL_SAVED_LD_LIBRARY_PATH=<unset>
env=0" "$got"

# 2. 元が「設定済みだが空」なら、空のまま戻す(未設定にしない)。
got=$(run_case "LD_LIBRARY_PATH" NIXGL_WRAPPED=1 LD_LIBRARY_PATH=/nix/mesa/lib __NIXGL_SAVED_LD_LIBRARY_PATH=)
expect "2 empty original stays set-but-empty" "LD_LIBRARY_PATH=
env=0" "$got"

# 3. 印が無ければ何もしない(`nixGLIntel zsh` を自分で打った場合など、nixGL を使う操作を壊さない)。
got=$(run_case "LD_LIBRARY_PATH LIBGL_DRIVERS_PATH" LD_LIBRARY_PATH=/nix/mesa/lib LIBGL_DRIVERS_PATH=/nix/mesa/dri)
expect "3 no mark: untouched" "LD_LIBRARY_PATH=/nix/mesa/lib
LIBGL_DRIVERS_PATH=/nix/mesa/dri
env=0" "$got"

# 4. 退避の無い変数は、印があっても触らない(一部だけ退避された場合)。
got=$(run_case "LD_LIBRARY_PATH LIBGL_DRIVERS_PATH" NIXGL_WRAPPED=1 \
    LD_LIBRARY_PATH=/nix/mesa/lib __NIXGL_SAVED_LD_LIBRARY_PATH=/orig/lib LIBGL_DRIVERS_PATH=/mine)
expect "4 variable without a saved value is left alone" "LD_LIBRARY_PATH=/orig/lib
LIBGL_DRIVERS_PATH=/mine
env=0" "$got"

# 5. 値に空白・引用符・特殊文字が入っていても、そのまま戻る。
got=$(run_case "LD_LIBRARY_PATH" NIXGL_WRAPPED=1 LD_LIBRARY_PATH=/nix/x \
    "__NIXGL_SAVED_LD_LIBRARY_PATH=/a b/\$c:'d'\"e\"")
expect "5 awkward characters survive" "LD_LIBRARY_PATH=/a b/\$c:'d'\"e\"
env=0" "$got"

(( fail )) && { print -r -- "nixgl-restore.test.zsh: FAILED"; exit 1; }
print -r -- "nixgl-restore.test.zsh: all ok"
