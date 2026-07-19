#!/usr/bin/env zsh
# 35-secrets-sops.zsh - Load secrets from SOPS-encrypted file
# Thin orchestration layer that calls wrapper script

# Only load in interactive shells
[[ ! -o interactive ]] && return 0

# Configuration paths
SOPS_DIR="${SOPS_DIR:-$HOME/.sops}"
SOPS_CONFIG="$SOPS_DIR/.sops.yaml"
SECRETS_FILE="$SOPS_DIR/.env"
# home-manager deploys sops-secrets-env.sh to ~/.local/bin (#212);
# fall back to the legacy dotfiles location for non-HM environments.
SOPS_SECRETS_SCRIPT="${SOPS_SECRETS_SCRIPT:-}"
if [[ -z "$SOPS_SECRETS_SCRIPT" ]]; then
    if [[ -x "$HOME/.local/bin/sops-secrets-env.sh" ]]; then
        SOPS_SECRETS_SCRIPT="$HOME/.local/bin/sops-secrets-env.sh"
    else
        SOPS_SECRETS_SCRIPT="${DOTFILES_DIR:-$HOME/dotfiles}/scripts/sops-secrets-env.sh"
    fi
fi

# Function to generate MCP-gdrive OAuth credentials file
_generate_mcp_gdrive_credentials() {
    # Skip if credentials not set or are placeholders
    [[ -z "${GDRIVE_CLIENT_ID:-}" ]] && return 0
    [[ -z "${GDRIVE_CLIENT_SECRET:-}" ]] && return 0
    [[ "$GDRIVE_CLIENT_ID" == *_here ]] && return 0
    [[ "$GDRIVE_CLIENT_SECRET" == *_here ]] && return 0

    # Set up credentials directory
    local gdrive_creds_dir="${XDG_DATA_HOME:-$HOME/.local/share}/mcp-gdrive"
    mkdir -p "$gdrive_creds_dir" 2>/dev/null
    chmod 700 "$gdrive_creds_dir" 2>/dev/null

    # Generate JSON securely using Python3
    if command -v python3 >/dev/null 2>&1; then
        GDRIVE_CLIENT_ID_VALUE="$GDRIVE_CLIENT_ID" \
        GDRIVE_CLIENT_SECRET_VALUE="$GDRIVE_CLIENT_SECRET" \
        python3 - "$gdrive_creds_dir/gcp-oauth.keys.json" << 'EOF'
import json, os, sys
data = {
    "installed": {
        "client_id": os.environ["GDRIVE_CLIENT_ID_VALUE"],
        "client_secret": os.environ["GDRIVE_CLIENT_SECRET_VALUE"],
        "redirect_uris": ["http://localhost"]
    }
}
with open(sys.argv[1], "w") as f:
    json.dump(data, f, indent=2)
EOF
        chmod 600 "$gdrive_creds_dir/gcp-oauth.keys.json" 2>/dev/null
    fi

    # Export credentials directory path
    export GDRIVE_CREDS_DIR="$gdrive_creds_dir"
}

# Function to load secrets from SOPS via wrapper script
load_sops_secrets() {
    # Check if wrapper script exists and is executable
    if [[ ! -x "$SOPS_SECRETS_SCRIPT" ]]; then
        return 1
    fi

    # Execute wrapper script and eval its output
    # Silent return on failure
    eval "$("$SOPS_SECRETS_SCRIPT" 2>/dev/null)"
    local result=$?

    # Generate MCP-gdrive credentials if secrets loaded successfully
    [[ $result -eq 0 ]] && _generate_mcp_gdrive_credentials

    return $result
}

# Function to manually reload secrets
reload_sops_secrets() {
    local show_values=false

    # Parse options
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --show-values)
                show_values=true
                shift
                ;;
            *)
                echo "Unknown option: $1"
                echo "Usage: reload_sops_secrets [--show-values]"
                return 1
                ;;
        esac
    done

    if [[ ! -x "$SOPS_SECRETS_SCRIPT" ]]; then
        echo "✗ Wrapper script not found or not executable: $SOPS_SECRETS_SCRIPT"
        return 1
    fi

    # Capture script output to count variables
    local script_output
    script_output=$("$SOPS_SECRETS_SCRIPT" 2>/dev/null)

    if [[ -n "$script_output" ]]; then
        # Execute exports
        eval "$script_output" 2>/dev/null

        # Generate MCP-gdrive credentials
        _generate_mcp_gdrive_credentials

        # Count loaded variables
        local count=$(echo "$script_output" | wc -l)

        echo "✓ Secrets loaded from SOPS"
        echo "  Variables exported ($count):"

        # Show variable names and values based on --show-values flag
        if [[ "$show_values" == true ]]; then
            echo "$script_output" | sed 's/^export /    /'
        else
            echo "$script_output" | sed 's/^export /    /' | sed 's/=.*/: [set]/'
        fi

        # Show GDRIVE_CREDS_DIR if set
        if [[ -n "${GDRIVE_CREDS_DIR:-}" ]]; then
            if [[ "$show_values" == true ]]; then
                echo "    GDRIVE_CREDS_DIR=$GDRIVE_CREDS_DIR"
            else
                echo "    GDRIVE_CREDS_DIR: [set]"
            fi
        fi

        return 0
    else
        echo "✗ Failed to load secrets from SOPS"
        echo "  Possible reasons:"
        echo "    - SOPS or GPG not installed"
        echo "    - Secrets file not found: $SECRETS_FILE"
        echo "    - Cannot decrypt (GPG key not available)"
        echo ""
        echo "  Setup instructions:"
        echo "    scripts/setup-sops-secrets.sh init"
        return 1
    fi
}

# Function to view current secrets status
sops_status() {
    local show_values=false

    # Parse options
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --show-values)
                show_values=true
                shift
                ;;
            *)
                echo "Unknown option: $1"
                echo "Usage: sops_status [--show-values]"
                return 1
                ;;
        esac
    done

    echo "=== SOPS Secrets Status ==="
    echo ""

    # Check tools
    echo "Tools:"
    if command -v sops &> /dev/null; then
        echo "  ✓ sops: $(sops --version 2>&1 | head -1)"
    else
        echo "  ✗ sops: not installed"
    fi

    if command -v gpg &> /dev/null; then
        echo "  ✓ gpg: $(gpg --version | head -1)"
    else
        echo "  ✗ gpg: not installed"
    fi

    echo ""

    # Check files
    echo "Configuration:"
    if [[ -f "$SOPS_CONFIG" ]]; then
        echo "  ✓ SOPS config: $SOPS_CONFIG"
    else
        echo "  ✗ SOPS config: not found"
    fi

    if [[ -f "$SECRETS_FILE" ]]; then
        echo "  ✓ Secrets file: $SECRETS_FILE"
    else
        echo "  ✗ Secrets file: not found"
    fi

    if [[ -x "$SOPS_SECRETS_SCRIPT" ]]; then
        echo "  ✓ Wrapper script: $SOPS_SECRETS_SCRIPT"
    else
        echo "  ✗ Wrapper script: not found or not executable"
    fi

    echo ""

    # Check environment variables loaded from secrets
    echo "Environment Variables:"
    if [[ -x "$SOPS_SECRETS_SCRIPT" ]]; then
        local script_output
        script_output=$("$SOPS_SECRETS_SCRIPT" 2>/dev/null)

        if [[ -n "$script_output" ]]; then
            local count=0
            local loaded_count=0

            # Parse variable names from script output
            while IFS= read -r line; do
                # Extract variable name and value from "export KEY=value"
                local var_name=$(echo "$line" | sed 's/^export //' | cut -d= -f1)
                local var_value=$(echo "$line" | sed 's/^export //' | cut -d= -f2-)

                if [[ -n "${(P)var_name}" ]]; then
                    if [[ "$show_values" == true ]]; then
                        echo "  ✓ $var_name: $var_value"
                    else
                        echo "  ✓ $var_name: [set]"
                    fi
                    ((loaded_count++))
                else
                    echo "  ✗ $var_name: [not set]"
                fi
                ((count++))
            done <<< "$script_output"

            # Show GDRIVE_CREDS_DIR status
            if [[ -n "${GDRIVE_CREDS_DIR:-}" ]]; then
                if [[ "$show_values" == true ]]; then
                    echo "  ✓ GDRIVE_CREDS_DIR: $GDRIVE_CREDS_DIR"
                else
                    echo "  ✓ GDRIVE_CREDS_DIR: [set]"
                fi
                ((loaded_count++))
                ((count++))

                # Check if gcp-oauth.keys.json exists
                local json_file="$GDRIVE_CREDS_DIR/gcp-oauth.keys.json"
                if [[ -f "$json_file" ]]; then
                    echo "  ✓ Google Drive OAuth JSON: $json_file"
                else
                    echo "  ✗ Google Drive OAuth JSON: not found"
                fi
            fi

            echo ""
            echo "Status: ✓ $loaded_count/$count variables loaded successfully"
        else
            echo "  ✗ No secrets available (cannot decrypt or no secrets defined)"
            echo ""
            echo "Status: ✗ Secrets not loaded"
            echo ""
            echo "Run: scripts/setup-sops-secrets.sh init"
        fi
    else
        echo "  ✗ Cannot check (wrapper script not available)"
        echo ""
        echo "Status: ✗ Wrapper script missing"
    fi
}

# Automatically load secrets on shell startup
load_sops_secrets

# Note: If loading fails, it fails silently to avoid cluttering shell startup.
# Use 'reload_sops_secrets' or 'sops_status' to debug if needed.
