#!/usr/bin/env bash
set -euo pipefail

# Host firewall for café-Wi-Fi safety (ADR-471): the same LAN-reachability
# boundary applies whether this host is at home or on a café network — there
# is no location-detection branch to keep in sync, by design.
#
# Linux (ufw): deny all incoming by default, allow only from the tailscale0
# interface plus Tailscale's own direct-connect port. Local subnets never
# reach an SSH/Syncthing/etc. listener on this host; the tailnet always can.
#
# darwin (Application Layer Firewall / ALF): ALF has no per-interface allow
# rule, so the equivalent boundary here is coarser — block ALL incoming
# connections, not just non-tailnet ones. Tailscale SSH server still answers
# over the tailnet because it terminates inside Tailscale.app's own userspace
# netstack, which ALF's socket-level filtering does not see (verify with the
# runbook's `ssh <this host>` check from another tailnet member after
# applying).
#
# Every step here is idempotent: re-running converges from any starting
# state, on café Wi-Fi or at home, with nothing to toggle by hand.
#
# Usage:
#   ./scripts/setup-firewall.sh [--dry-run]

DRY_RUN=false

usage() {
    cat <<'EOF'
Usage: setup-firewall.sh [--dry-run]

Apply the host firewall boundary described in docs/adr/471-cafe-wifi-
tailscale-mesh-and-host-firewall.md: deny LAN-side inbound reachability the
same way on every network, so the boundary needs no per-location switch.

Options:
    --dry-run   Print what would run; touch nothing, never invoke sudo
    -h, --help  Show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Error: unknown option: $1" >&2
            exit 1
            ;;
    esac
done

fail() {
    echo "Error: $*" >&2
    exit 1
}

setup_linux() {
    local -a plan=(
        "ufw default deny incoming"
        "ufw default allow outgoing"
        "ufw allow in on tailscale0"
        "ufw allow 41641/udp"
        "ufw --force enable"
    )

    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[dry-run] Linux (ufw) — would run:"
        printf '  sudo %s\n' "${plan[@]}"
        return 0
    fi

    command -v ufw >/dev/null 2>&1 || fail "ufw not found — see packages/declarative/apt-packages.txt"

    echo "Applying ufw rules..."
    sudo ufw default deny incoming
    sudo ufw default allow outgoing
    sudo ufw allow in on tailscale0
    sudo ufw allow 41641/udp
    sudo ufw --force enable

    echo "ufw status:"
    sudo ufw status verbose
}

setup_darwin() {
    local socketfilterfw="/usr/libexec/ApplicationFirewall/socketfilterfw"
    local -a plan=(
        "--setglobalstate on"
        "--setblockall on"
        "--setstealthmode on"
    )

    if [[ "$DRY_RUN" == "true" ]]; then
        echo "[dry-run] darwin (ALF) — would run:"
        printf '  sudo %s %s\n' "$socketfilterfw" "${plan[@]}"
        return 0
    fi

    [[ -x "$socketfilterfw" ]] || fail "$socketfilterfw not found"

    echo "Applying Application Layer Firewall settings..."
    sudo "$socketfilterfw" --setglobalstate on
    sudo "$socketfilterfw" --setblockall on
    sudo "$socketfilterfw" --setstealthmode on

    echo "ALF status:"
    "$socketfilterfw" --getglobalstate
    "$socketfilterfw" --getblockall
    "$socketfilterfw" --getstealthmode
}

case "$(uname -s)" in
    Linux)
        setup_linux
        ;;
    Darwin)
        setup_darwin
        ;;
    *)
        fail "unsupported platform: $(uname -s)"
        ;;
esac

echo "Done."
