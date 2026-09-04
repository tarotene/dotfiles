#!/bin/bash
# Shared identity checks for the global Git hooks.  This intentionally reads
# only the global config: a repository-local identity must not silently change
# who creates commits or release tags from this machine.
set -euo pipefail

die() {
    echo "error: Git identity guard: $*" >&2
    exit 1
}

global_identity() {
    local name email
    name=$(git config --global --get user.name || true)
    email=$(git config --global --get user.email || true)
    [[ -n "$name" && -n "$email" ]] || die "global user.name and user.email must both be configured"
    printf '%s\t%s\n' "$name" "$email"
}

identity_parts() {
    local ident=$1 name email
    name=${ident%% <*}
    email=${ident#*<}
    email=${email%%>*}
    [[ "$ident" == *' <'*'>'* && -n "$name" && -n "$email" ]] || die "could not parse Git identity: $ident"
    printf '%s\t%s\n' "$name" "$email"
}

require_effective_identity() {
    local expected actual expected_name expected_email actual_name actual_email
    expected=$(global_identity)
    actual=$(identity_parts "$(git var "$1")")
    IFS=$'\t' read -r expected_name expected_email <<<"$expected"
    IFS=$'\t' read -r actual_name actual_email <<<"$actual"
    [[ "$actual_name" == "$expected_name" && "$actual_email" == "$expected_email" ]] ||
        die "$1 is '$actual_name <$actual_email>', expected '$expected_name <$expected_email>' from global Git config"
}

require_tagger_identity() {
    local oid=$1 expected actual expected_name expected_email actual_name actual_email
    expected=$(global_identity)
    actual=$(git for-each-ref --format='%(taggername)%09%(taggeremail)' "refs/tags/$2")
    IFS=$'\t' read -r expected_name expected_email <<<"$expected"
    IFS=$'\t' read -r actual_name actual_email <<<"$actual"
    [[ -n "$actual_name" && -n "$actual_email" ]] || die "tag '$2' must be annotated"
    [[ "$actual_name" == "$expected_name" && "$actual_email" == "<$expected_email>" ]] ||
        die "tag '$2' has tagger '$actual_name $actual_email', expected '$expected_name <$expected_email>' from global Git config"
}

require_release_signature() {
    local oid=$1 ref=$2 expected output actual
    expected=$(git config --global --get user.signingkey || true)
    [[ -n "$expected" ]] || die "release tag '$ref' requires global user.signingkey"
    output=$(git verify-tag --raw "$oid" 2>&1) || die "release tag '$ref' has no verifiable OpenPGP signature"
    actual=$(sed -n 's/^\[GNUPG:\] VALIDSIG \([^ ]*\).*/\1/p' <<<"$output" | head -n1)
    [[ -n "$actual" ]] || die "could not read signing fingerprint for release tag '$ref'"
    expected=${expected// /}
    [[ "${actual^^}" == "${expected^^}" ]] ||
        die "release tag '$ref' is signed by $actual, expected $expected from global Git config"
}

case "${1:-}" in
    commit)
        require_effective_identity GIT_AUTHOR_IDENT
        require_effective_identity GIT_COMMITTER_IDENT
        ;;
    tag)
        [[ $# -eq 3 ]] || die "internal use requires: tag <object-id> <tag-name>"
        oid=$2
        tag_name=$3
        [[ "$(git cat-file -t "$oid" 2>/dev/null || true)" == tag ]] || {
            [[ "$tag_name" != verrine/* ]] && exit 0
            die "release tag '$tag_name' must be annotated"
        }
        require_tagger_identity "$oid" "$tag_name"
        if [[ "$tag_name" == verrine/* ]]; then
            require_release_signature "$oid" "$tag_name"
        fi
        ;;
    *)
        die "internal use requires commit or tag mode"
        ;;
esac
