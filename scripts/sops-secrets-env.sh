#!/usr/bin/env bash
# SOPS Secrets Environment Variable Exporter
# Outputs export statements for all secrets in SOPS-encrypted dotenv file
#
# Usage:
#   source <(./scripts/sops-secrets-env.sh)   # Load into current shell
#   ./scripts/sops-secrets-env.sh             # View export statements

set -euo pipefail

# Configuration paths
SOPS_DIR="${SOPS_DIR:-$HOME/.sops}"
SOPS_CONFIG="$SOPS_DIR/.sops.yaml"
SECRETS_FILE="$SOPS_DIR/.env"

# Silent exit if prerequisites missing
command -v sops &>/dev/null || exit 0
command -v gpg &>/dev/null || exit 0
[[ -f "$SECRETS_FILE" ]] || exit 0
[[ -f "$SOPS_CONFIG" ]] || exit 0

# Decrypt secrets (silent exit on failure)
secrets=$(sops --config "$SOPS_CONFIG" --decrypt "$SECRETS_FILE" 2>/dev/null) || exit 0

# Parse dotenv format and output export statements
while IFS='=' read -r key value; do
    # Skip empty lines and comments
    [[ -z "$key" || "$key" =~ ^# ]] && continue

    # Skip placeholder values (not set yet)
    [[ "$value" == *_here ]] && continue

    # Output properly quoted export statement
    printf 'export %s=%q\n' "$key" "$value"
done <<< "$secrets"
