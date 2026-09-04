#!/bin/bash
# Run from any checkout: config/git/hooks/identity-guard-self-test.sh
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
guard="$script_dir/identity-guard.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

global_config="$tmp/global.gitconfig"
repo="$tmp/repo"
git init -q "$repo"
git -C "$repo" config --file "$global_config" user.name 'Kentaro Sugimoto'
git -C "$repo" config --file "$global_config" user.email 'sugimoto-kentaro@arkedgespace.com'

run_guard() {
    GIT_CONFIG_GLOBAL="$global_config" git -C "$repo" -c core.hooksPath=/dev/null "$@"
}

# The guard itself invokes git, so run it from inside the fixture repository.
(
    cd "$repo"
    export GIT_CONFIG_GLOBAL="$global_config"
    "$guard" commit
)

git -C "$repo" config user.name test
git -C "$repo" config user.email test@example.com
if (cd "$repo" && GIT_CONFIG_GLOBAL="$global_config" "$guard" commit); then
    echo 'identity guard self-test: expected local test identity to be rejected' >&2
    exit 1
fi
git -C "$repo" config --unset-all user.name
git -C "$repo" config --unset-all user.email

if (cd "$repo" && GIT_CONFIG_GLOBAL="$global_config" GIT_AUTHOR_EMAIL=test@example.com "$guard" commit); then
    echo 'identity guard self-test: expected author environment override to be rejected' >&2
    exit 1
fi

GIT_ALLOW_MAIN_COMMIT=1 git -C "$repo" -c commit.gpgsign=false -c user.name='Kentaro Sugimoto' -c user.email='sugimoto-kentaro@arkedgespace.com' \
    commit --allow-empty -q -m fixture
git -C "$repo" config user.name test
git -C "$repo" config user.email test@example.com
git -C "$repo" -c tag.gpgsign=false tag -a test-tag -m fixture
if (cd "$repo" && GIT_CONFIG_GLOBAL="$global_config" "$guard" tag "$(git rev-parse test-tag)" test-tag); then
    echo 'identity guard self-test: expected test tagger to be rejected' >&2
    exit 1
fi

echo 'identity guard self-test: passed'
