#!/usr/bin/env bash
set -euo pipefail

# Install the company-provided CrowdStrike Falcon Sensor package on the new
# company Pop!_OS host. The package and CID are deliberately kept outside Git.
#
# Every step is idempotent: re-running the same command converges on a
# registered, running sensor from any interrupted state.
#
# Usage:
#   ./scripts/install-falcon-sensor.sh --package <sensor.deb>
#   ./scripts/install-falcon-sensor.sh --dry-run --package <sensor.deb>

TARGET_HOST="company-pop-new"
TARGET_OS="pop"
TARGET_VERSION="24.04"
TARGET_ARCH="amd64"
FALCONCTL="/opt/CrowdStrike/falconctl"

PACKAGE_PATH=""
DRY_RUN=false
WORK_DIR=""

package_name=""
package_arch=""
package_version=""

usage() {
    cat <<'EOF'
Usage: install-falcon-sensor.sh --package <sensor.deb> [--dry-run]

Install and register CrowdStrike Falcon Sensor on company-pop-new.

Options:
    --package <path>  Company-provided falcon-sensor .deb package
    --dry-run         Run every check but change nothing on the host
    -h, --help        Show this help

Environment:
    FALCON_CID        CrowdStrike Customer ID.  When unset it is read from
                      host-local SOPS via ~/.local/bin/sops-secrets-env
                      (requires the YubiKey), so it never has to be typed.
EOF
}

fail() {
    echo "Error: $*" >&2
    exit 1
}

warn() {
    echo "Warning: $*" >&2
}

cleanup() {
    [[ -n "$WORK_DIR" && -d "$WORK_DIR" ]] && rm -rf "$WORK_DIR"
    return 0
}
trap cleanup EXIT

on_error() {
    local line=$1

    echo "Falcon Sensor installation failed near line $line." >&2
    show_diagnostics
}

show_diagnostics() {
    echo "Inspect the service with:" >&2
    echo "  sudo systemctl status falcon-sensor --no-pager" >&2
    echo "  sudo journalctl -u falcon-sensor -n 50 --no-pager" >&2
}

runtime_fail() {
    echo "Error: $*" >&2
    show_diagnostics
    exit 1
}

# A host mismatch is fatal for a real installation, but only a warning under
# --dry-run so the rehearsal still exercises the package checks off-host (CI
# runs the dry-run on a GitHub runner).
host_problem() {
    if [[ "$DRY_RUN" == true ]]; then
        warn "$*"
    else
        fail "$*"
    fi
}

require_commands() {
    local command_name

    for command_name in "$@"; do
        command -v "$command_name" >/dev/null 2>&1 \
            || fail "required command not found: $command_name"
    done
}

validate_host() {
    [[ "$(hostname -s)" == "$TARGET_HOST" ]] \
        || host_problem "this installer only supports $TARGET_HOST, found $(hostname -s)"

    if [[ -r /etc/os-release ]]; then
        # shellcheck disable=SC1091
        source /etc/os-release
        [[ "${ID:-}" == "$TARGET_OS" ]] \
            || host_problem "expected Pop!_OS (ID=$TARGET_OS), found ${ID:-unknown}"
        [[ "${VERSION_ID:-}" == "$TARGET_VERSION" ]] \
            || host_problem "expected Pop!_OS $TARGET_VERSION, found ${VERSION_ID:-unknown}"
    else
        host_problem "/etc/os-release is not readable"
    fi

    [[ "$(dpkg --print-architecture)" == "$TARGET_ARCH" ]] \
        || host_problem "expected $TARGET_ARCH architecture, found $(dpkg --print-architecture)"
}

# Always fatal, including under --dry-run: a rehearsal that does not read the
# package metadata proves nothing about the real run.
validate_package() {
    [[ -f "$PACKAGE_PATH" ]] || fail "package not found: $PACKAGE_PATH"
    PACKAGE_PATH=$(realpath "$PACKAGE_PATH")

    package_name=$(dpkg-deb -f "$PACKAGE_PATH" Package)
    package_arch=$(dpkg-deb -f "$PACKAGE_PATH" Architecture)
    package_version=$(dpkg-deb -f "$PACKAGE_PATH" Version)

    [[ "$package_name" == "falcon-sensor" ]] \
        || fail "expected package falcon-sensor, found $package_name"
    [[ "$package_arch" == "$TARGET_ARCH" ]] \
        || fail "expected package architecture $TARGET_ARCH, found $package_arch"
}

# Read FALCON_CID from host-local SOPS so it never has to be typed into an
# interactive shell (where it would land in the shell history).  The helper
# prints `export KEY=<quoted value>` for every secret; take only FALCON_CID so
# the rest never reaches this process environment.
load_cid_from_sops() {
    local helper="$HOME/.local/bin/sops-secrets-env"
    local line

    [[ -x "$helper" ]] || return 1
    # Bounded on purpose.  With the YubiKey absent gpg can block on the card
    # rather than fail, and the helper carries no timeout of its own, so an
    # unbounded call can hang the installer.  The hint below is more useful.
    line=$(timeout 30 "$helper" 2>/dev/null | grep '^export FALCON_CID=') || return 1
    eval "$line"
    [[ -n "${FALCON_CID:-}" ]]
}

resolve_cid() {
    [[ -n "${FALCON_CID:-}" ]] && return 0
    load_cid_from_sops
}

cid_hint() {
    if [[ ! -d "$HOME/.sops" ]]; then
        echo "Host-local SOPS is not initialised on this host. Run:" >&2
        echo "  ./scripts/setup-sops-secrets.sh init" >&2
        echo "  ./scripts/setup-sops-secrets.sh add-secret FALCON_CID" >&2
    else
        echo "Host-local SOPS exists but FALCON_CID could not be read from it." >&2
        echo "Check that the YubiKey is inserted:" >&2
        echo "  gpg --card-status" >&2
        echo "then confirm the secret is present:" >&2
        echo "  ./scripts/setup-sops-secrets.sh add-secret FALCON_CID" >&2
    fi
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --package)
            [[ $# -ge 2 ]] || fail "--package requires a path"
            PACKAGE_PATH=$2
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "unknown option: $1"
            ;;
    esac
done

[[ -n "$PACKAGE_PATH" ]] || fail "--package is required"

require_commands hostname realpath dpkg dpkg-deb timeout
validate_host
validate_package

if [[ "$DRY_RUN" == true ]]; then
    cid_status="[not available; required for installation]"
    resolve_cid && cid_status="[available]"

    cat <<EOF
[dry-run] Validated:
  host: $(hostname -s) (expected $TARGET_HOST)
  OS: ${PRETTY_NAME:-unknown}
  architecture: $(dpkg --print-architecture)
  package: $PACKAGE_PATH ($package_name $package_version, $package_arch)
  FALCON_CID: $cid_status
[dry-run] Would install the staged package copy with apt-get.
[dry-run] Would configure the CID with falconctl.
[dry-run] Would enable, start, and verify falcon-sensor.service.
EOF
    exit 0
fi

require_commands dpkg-query apt-get systemctl mktemp sudo

resolve_cid || {
    echo "Error: FALCON_CID is not available." >&2
    cid_hint
    exit 1
}

echo "Validated Falcon Sensor package $package_version ($package_arch)."
echo "Requesting sudo authorization..."
sudo -v

trap 'on_error "$LINENO"' ERR

# apt cannot read a package under $HOME as the unprivileged _apt user, which
# disables its download sandbox.  Stage the package in a root-readable
# directory (on disk, not tmpfs — the sensor package is ~90 MB) so the caller
# can keep the .deb wherever they like.
WORK_DIR=$(mktemp -d /var/tmp/falcon-sensor.XXXXXX)
chmod 0755 "$WORK_DIR"
staged_package="$WORK_DIR/${PACKAGE_PATH##*/}"
cp "$PACKAGE_PATH" "$staged_package"
chmod 0644 "$staged_package"

echo "Installing Falcon Sensor package..."
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "$staged_package"

# /opt/CrowdStrike is 0750 root:root, so an unprivileged test cannot even
# traverse it — this check must run as root or it always reports "missing".
sudo test -x "$FALCONCTL" || runtime_fail "$FALCONCTL was not installed"

# -f is required whenever a CID is already set: without it falconctl refuses
# with "CID is set, but -f was not specified".  Every re-run and every sensor
# update hits that, so the plain form is only ever correct on a virgin host.
echo "Registering Falcon Sensor with the company tenant..."
sudo "$FALCONCTL" -s -f --cid="$FALCON_CID"

# The package postinst enables and starts the unit before any CID exists, so a
# fresh install always leaves one failed start behind.  Clearing it (and the
# start rate limit) is what lets this script converge on a re-run.
echo "Enabling and starting falcon-sensor.service..."
sudo systemctl reset-failed falcon-sensor.service || true
sudo systemctl enable --now falcon-sensor.service

installed_status=$(dpkg-query -W -f='${Status}' falcon-sensor)
[[ "$installed_status" == "install ok installed" ]] || runtime_fail "falcon-sensor package is not fully installed"
sudo systemctl is-enabled --quiet falcon-sensor.service
# The unit is Type=forking with a PIDFile, so is-active already tracks the
# falcond MainPID; confirm registration rather than matching a process name.
sudo systemctl is-active --quiet falcon-sensor.service
# Both streams are discarded because falconctl echoes the CID back.
sudo "$FALCONCTL" -g --cid >/dev/null 2>&1 \
    || runtime_fail "falconctl reports no CID after registration"

trap - ERR

echo "Falcon Sensor $package_version is installed and running."
echo "Ask the company IT administrator to confirm this host in Newly Installed Sensors."
