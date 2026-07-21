#!/bin/bash
set -euo pipefail

# Install the company-provided CrowdStrike Falcon Sensor package on the new
# company Pop!_OS host. The package and CID are deliberately kept outside Git.
#
# Usage:
#   FALCON_CID=... ./scripts/install-falcon-sensor.sh --package <sensor.deb>
#   ./scripts/install-falcon-sensor.sh --dry-run --package <sensor.deb>

TARGET_HOST="company-pop-new"
TARGET_OS="pop"
TARGET_VERSION="24.04"
TARGET_ARCH="amd64"

PACKAGE_PATH=""
DRY_RUN=false

usage() {
    cat <<'EOF'
Usage: install-falcon-sensor.sh --package <sensor.deb> [--dry-run]

Install and register CrowdStrike Falcon Sensor on company-pop-new.

Options:
    --package <path>  Company-provided falcon-sensor .deb package
    --dry-run         Print the planned checks and actions without changing the host
    -h, --help        Show this help

Environment:
    FALCON_CID        CrowdStrike Customer ID loaded from host-local SOPS
EOF
}

fail() {
    echo "Error: $*" >&2
    exit 1
}

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

if [[ "$DRY_RUN" == true ]]; then
    cid_status="[not set; required for installation]"
    [[ -n "${FALCON_CID:-}" ]] && cid_status="[set]"

    cat <<EOF
[dry-run] Would validate:
  host: $TARGET_HOST
  OS: Pop!_OS $TARGET_VERSION
  architecture: $TARGET_ARCH
  package: $PACKAGE_PATH (falcon-sensor, $TARGET_ARCH)
  FALCON_CID: $cid_status
[dry-run] Would install the local package with apt-get.
[dry-run] Would configure the CID with falconctl.
[dry-run] Would enable, start, and verify falcon-sensor.service.
EOF
    exit 0
fi

[[ -n "${FALCON_CID:-}" ]] || fail "FALCON_CID is not set; load it from host-local SOPS"

for command_name in hostname realpath dpkg dpkg-deb dpkg-query apt-get systemctl pgrep sudo; do
    command -v "$command_name" >/dev/null 2>&1 || fail "required command not found: $command_name"
done

[[ "$(hostname -s)" == "$TARGET_HOST" ]] || fail "this installer only supports $TARGET_HOST"
[[ -r /etc/os-release ]] || fail "/etc/os-release is not readable"

# shellcheck disable=SC1091
source /etc/os-release
[[ "${ID:-}" == "$TARGET_OS" ]] || fail "expected Pop!_OS (ID=$TARGET_OS), found ${ID:-unknown}"
[[ "${VERSION_ID:-}" == "$TARGET_VERSION" ]] || fail "expected Pop!_OS $TARGET_VERSION, found ${VERSION_ID:-unknown}"
[[ "$(dpkg --print-architecture)" == "$TARGET_ARCH" ]] || fail "expected $TARGET_ARCH architecture"

[[ -f "$PACKAGE_PATH" ]] || fail "package not found: $PACKAGE_PATH"
PACKAGE_PATH=$(realpath "$PACKAGE_PATH")

package_name=$(dpkg-deb -f "$PACKAGE_PATH" Package)
package_arch=$(dpkg-deb -f "$PACKAGE_PATH" Architecture)
package_version=$(dpkg-deb -f "$PACKAGE_PATH" Version)

[[ "$package_name" == "falcon-sensor" ]] || fail "expected package falcon-sensor, found $package_name"
[[ "$package_arch" == "$TARGET_ARCH" ]] || fail "expected package architecture $TARGET_ARCH, found $package_arch"

echo "Validated Falcon Sensor package $package_version ($package_arch)."
echo "Requesting sudo authorization..."
sudo -v

trap 'on_error "$LINENO"' ERR

echo "Installing Falcon Sensor package..."
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y "$PACKAGE_PATH"

[[ -x /opt/CrowdStrike/falconctl ]] || runtime_fail "/opt/CrowdStrike/falconctl was not installed"

echo "Registering Falcon Sensor with the company tenant..."
sudo /opt/CrowdStrike/falconctl -s --cid="$FALCON_CID"

echo "Enabling and starting falcon-sensor.service..."
sudo systemctl enable --now falcon-sensor.service

installed_status=$(dpkg-query -W -f='${Status}' falcon-sensor)
[[ "$installed_status" == "install ok installed" ]] || runtime_fail "falcon-sensor package is not fully installed"
sudo systemctl is-enabled --quiet falcon-sensor.service
sudo systemctl is-active --quiet falcon-sensor.service
sudo pgrep -x falcon-sensor >/dev/null

trap - ERR

echo "Falcon Sensor $package_version is installed and running."
echo "Ask the company IT administrator to confirm this host in Newly Installed Sensors."
