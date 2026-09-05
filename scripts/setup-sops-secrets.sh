#!/usr/bin/env bash
# SOPS Secrets Setup Script
# Creates and manages encrypted secrets using SOPS with GPG

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Configuration paths
SOPS_DIR="${SOPS_DIR:-$HOME/.sops}"
SOPS_CONFIG="$SOPS_DIR/.sops.yaml"
SECRETS_FILE="$SOPS_DIR/.env"

# Dry-run mode flag
DRY_RUN=false

# Non-interactive mode flag
YES_MODE=false

# Print colored messages
print_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_dry_run() {
    echo -e "${CYAN}[DRY RUN]${NC} $1"
}

# Show usage
usage() {
    cat << EOF
Usage: $0 [OPTIONS] [COMMAND]

Set up and manage encrypted secrets using SOPS with GPG encryption.

COMMANDS:
    init               Initialize SOPS configuration (default)
    edit               Edit secrets file with SOPS
    view               View decrypted secrets
    add-secret <KEY>   Interactively add a secret by key name
    validate           Validate configuration and GPG setup
    help               Show this help message

OPTIONS:
    -d, --dry-run   Dry-run mode (show what would be done)
    -y, --yes       Non-interactive mode (automatically answer yes to prompts)
    -h, --help      Show this help message

EXAMPLES:
    $0 init                           # Initialize SOPS configuration
    $0 --dry-run init                 # Show what would be done
    $0 --yes init                     # Initialize without prompts (non-interactive)
    $0 edit                           # Edit secrets file
    $0 add-secret GITHUB_ACCESS_TOKEN # Add GitHub token interactively
    $0 add-secret MY_API_KEY          # Add custom secret interactively
    $0 view                           # View current secrets

SECURITY NOTES:
    - Secrets are encrypted with your GPG key
    - Original GPG key is required to decrypt
    - Keep your GPG key backed up securely
    - Never commit decrypted secrets to git

FILES:
    $SOPS_DIR/.sops.yaml    # SOPS configuration
    $SOPS_DIR/.env          # Encrypted secrets file

EOF
}

# Check prerequisites
check_prerequisites() {
    local missing_tools=()

    # Check for sops
    if ! command -v sops &> /dev/null; then
        missing_tools+=("sops")
    fi

    # Check for gpg
    if ! command -v gpg &> /dev/null; then
        missing_tools+=("gpg")
    fi

    if [ ${#missing_tools[@]} -gt 0 ]; then
        print_error "Missing required tools: ${missing_tools[*]}"
        echo ""
        echo "Install missing tools:"
        for tool in "${missing_tools[@]}"; do
            case "$tool" in
                sops)
                    echo "  - Run: ./scripts/install-packages.sh (sops is included)"
                    ;;
                gpg)
                    echo "  - Run: sudo apt install gnupg"
                    ;;
            esac
        done
        return 1
    fi

    return 0
}

# Get GPG key fingerprint
get_gpg_key() {
    local key_fingerprint

    # Read the fingerprint from the machine-readable listing.  The
    # human-readable one changes shape depending on gpg.conf: with
    # `with-fingerprint` set, the line after `sec` becomes
    # "Key fingerprint = ..." and scraping it yields "Keyfingerprint=<hex>",
    # which then makes every later gpg call fail.
    # The first fpr record belongs to the first secret key's primary key.
    key_fingerprint=$(gpg --list-secret-keys --with-colons 2>/dev/null |
        awk -F: '$1 == "fpr" { print $10; exit }')

    if [ -z "$key_fingerprint" ]; then
        print_error "No GPG secret key found"
        echo ""
        echo "Generate a GPG key first:"
        echo "  gpg --full-generate-key"
        return 1
    fi

    echo "$key_fingerprint"
}

# Initialize SOPS configuration
init_sops() {
    echo "=== SOPS Secrets Initialization ==="
    echo ""

    # Check prerequisites
    if ! check_prerequisites; then
        return 1
    fi

    # Get GPG key
    print_info "Detecting GPG key..."
    local gpg_key
    if ! gpg_key=$(get_gpg_key); then
        return 1
    fi
    print_success "Found GPG key: $gpg_key"

    # Show GPG key details
    echo ""
    echo "GPG Key Details:"
    echo "----------------"
    gpg --list-secret-keys --keyid-format=long "$gpg_key" 2>/dev/null | grep -E "^(sec|uid)" | sed 's/^/  /'
    echo ""

    # Create SOPS directory
    if [ "$DRY_RUN" = true ]; then
        print_dry_run "Would create directory: $SOPS_DIR"
    else
        if [ ! -d "$SOPS_DIR" ]; then
            print_info "Creating SOPS directory: $SOPS_DIR"
            mkdir -p "$SOPS_DIR"
            chmod 700 "$SOPS_DIR"
        else
            print_info "SOPS directory already exists: $SOPS_DIR"
        fi
    fi

    # Create SOPS configuration
    if [ -f "$SOPS_CONFIG" ] && [ "$DRY_RUN" = false ]; then
        print_warning "SOPS configuration already exists: $SOPS_CONFIG"

        if [ "$YES_MODE" = false ]; then
            echo ""
            read -p "Do you want to overwrite it? (y/N): " -n 1 -r
            echo ""
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                print_info "Keeping existing configuration"
                # Skip to secrets file creation if config is kept
                SKIP_CONFIG=true
            fi
        else
            print_info "Auto-confirmed: Overwriting configuration (--yes mode)"
        fi
    fi

    # Only create/update config if not skipping
    if [ "${SKIP_CONFIG:-false}" != "true" ]; then
        if [ "$DRY_RUN" = true ]; then
            print_dry_run "Would create SOPS configuration: $SOPS_CONFIG"
            echo ""
            echo "Configuration content:"
            echo "----------------------"
            cat << EOF
creation_rules:
  - path_regex: \.env$
    pgp: >-
      $gpg_key
EOF
        else
            print_info "Creating SOPS configuration: $SOPS_CONFIG"
            cat > "$SOPS_CONFIG" << EOF
# SOPS Configuration
# Generated on $(date)
# This file configures SOPS to use your GPG key for encryption

creation_rules:
  - path_regex: \.env$
    pgp: >-
      $gpg_key
EOF
            chmod 600 "$SOPS_CONFIG"
            print_success "SOPS configuration created"
        fi
    fi

    # Create initial secrets file if it doesn't exist
    if [ ! -f "$SECRETS_FILE" ] || [ "$DRY_RUN" = true ]; then
        if [ "$DRY_RUN" = true ]; then
            print_dry_run "Would create secrets file: $SECRETS_FILE"
            echo ""
            echo "Initial content (unencrypted template):"
            echo "-----------------------------------------"
            cat << EOF
# Add secrets in KEY=value format
# Example: MY_SECRET=secret_value
# Example: API_TOKEN=my_api_token_here
EOF
        else
            print_info "Creating initial secrets file..."

            # Create temporary unencrypted file with .env extension
            # NOTE: Using /tmp/secrets.XXXXXX.env pattern (works on Pop!_OS/Ubuntu 24.04)
            local temp_file
            temp_file=$(mktemp /tmp/secrets.XXXXXX.env)
            cat > "$temp_file" << EOF
# Add secrets in KEY=value format
# Example: MY_SECRET=secret_value
# Example: API_TOKEN=my_api_token_here
EOF

            # Encrypt with SOPS
            if sops --config "$SOPS_CONFIG" --encrypt --input-type dotenv --output-type dotenv --output "$SECRETS_FILE" "$temp_file"; then
                rm -f "$temp_file"
                chmod 600 "$SECRETS_FILE"
                print_success "Secrets file created (encrypted)"
            else
                print_error "Failed to encrypt secrets file"
                rm -f "$temp_file"
                return 1
            fi
        fi
    else
        print_info "Secrets file already exists: $SECRETS_FILE"
    fi

    echo ""
    print_success "SOPS initialization complete!"
    echo ""
    echo "Next steps:"
    echo "  1. Edit secrets file:"
    echo "     $0 edit"
    echo ""
    echo "  2. Or add secrets interactively:"
    echo "     $0 add-secret <KEY_NAME>"
    echo ""
    echo "  3. View secrets:"
    echo "     $0 view"
    echo ""
    echo "  4. All secrets will be automatically loaded in new Zsh shells"
    echo "  5. Or load manually: source <(./scripts/sops-secrets-env.sh)"
}

# Edit secrets file
edit_secrets() {
    if [ ! -f "$SECRETS_FILE" ]; then
        print_error "Secrets file not found: $SECRETS_FILE"
        echo "Run: $0 init"
        return 1
    fi

    if [ "$DRY_RUN" = true ]; then
        print_dry_run "Would edit secrets file with SOPS"
        return 0
    fi

    print_info "Opening secrets file for editing..."
    sops --config "$SOPS_CONFIG" "$SECRETS_FILE"
}

# View secrets
view_secrets() {
    if [ ! -f "$SECRETS_FILE" ]; then
        print_error "Secrets file not found: $SECRETS_FILE"
        echo "Run: $0 init"
        return 1
    fi

    if [ "$DRY_RUN" = true ]; then
        print_dry_run "Would decrypt and display secrets"
        return 0
    fi

    print_info "Decrypting secrets..."
    echo ""
    sops --config "$SOPS_CONFIG" --decrypt "$SECRETS_FILE"
}

# Add secret interactively (generic)
add_secret() {
    local key_name="$1"

    if [ -z "$key_name" ]; then
        print_error "Usage: $0 add-secret <KEY_NAME>"
        echo ""
        echo "Examples:"
        echo "  $0 add-secret GITHUB_ACCESS_TOKEN"
        echo "  $0 add-secret MY_API_KEY"
        return 1
    fi

    # Validate key_name follows environment variable naming conventions
    if ! [[ "$key_name" =~ ^[A-Z_][A-Z0-9_]*$ ]]; then
        print_error "Invalid KEY_NAME: '$key_name'"
        echo "KEY_NAME must use uppercase letters, numbers, and underscores only."
        echo "Cannot start with a number."
        echo ""
        echo "Examples of valid names:"
        echo "  GITHUB_ACCESS_TOKEN"
        echo "  MY_API_KEY"
        return 1
    fi

    if [ ! -f "$SECRETS_FILE" ]; then
        print_error "Secrets file not found: $SECRETS_FILE"
        echo "Run: $0 init first"
        return 1
    fi

    if [ "$DRY_RUN" = true ]; then
        print_dry_run "Would interactively add $key_name to secrets"
        return 0
    fi

    echo "=== Add Secret: $key_name ==="
    echo ""

    read -sp "Enter value for $key_name: " secret_value
    echo ""

    if [ -z "$secret_value" ]; then
        print_error "No value provided"
        return 1
    fi

    # Decrypt, update, and re-encrypt
    print_info "Updating secrets file..."

    # NOTE: Using /tmp/secrets.XXXXXX.env pattern (works on Pop!_OS/Ubuntu 24.04)
    local temp_file
    temp_file=$(mktemp /tmp/secrets.XXXXXX.env)

    # Decrypt to temp file
    if ! sops --config "$SOPS_CONFIG" --decrypt "$SECRETS_FILE" > "$temp_file"; then
        print_error "Failed to decrypt secrets file"
        rm -f "$temp_file"
        return 1
    fi

    # Update secret in temp file using safer approach
    # Create a new temp file with the updated secret
    # NOTE: Using /tmp/secrets-new.XXXXXX.env pattern (works on Pop!_OS/Ubuntu 24.04)
    local temp_file_new
    temp_file_new=$(mktemp /tmp/secrets-new.XXXXXX.env)

    # Check if key exists using safe string comparison
    local key_exists=false
    while IFS='=' read -r existing_key _; do
        if [[ "$existing_key" == "$key_name" ]]; then
            key_exists=true
            break
        fi
    done < "$temp_file"

    if [[ "$key_exists" == "true" ]]; then
        # Replace existing secret line
        while IFS= read -r line; do
            if [[ "${line%%=*}" == "$key_name" ]]; then
                echo "${key_name}=${secret_value}"
            else
                echo "$line"
            fi
        done < "$temp_file" > "$temp_file_new"
        mv "$temp_file_new" "$temp_file"
    else
        # Add new secret
        echo "${key_name}=${secret_value}" >> "$temp_file"
    fi

    # Re-encrypt
    if sops --config "$SOPS_CONFIG" --encrypt --input-type dotenv --output-type dotenv "$temp_file" > "$SECRETS_FILE"; then
        rm -f "$temp_file"
        chmod 600 "$SECRETS_FILE"
        print_success "$key_name added successfully"
    else
        print_error "Failed to encrypt updated secrets"
        rm -f "$temp_file"
        return 1
    fi

    echo ""
    print_success "Secret has been securely stored"
    echo "All secrets will be automatically loaded in new Zsh shells"
    echo "Or load manually: source <(./scripts/sops-secrets-env.sh)"

    # Clear sensitive value from shell memory
    unset secret_value
}

# Validate configuration
validate_config() {
    echo "=== SOPS Configuration Validation ==="
    echo ""

    local validation_passed=true

    # Check prerequisites
    print_info "Checking prerequisites..."
    if check_prerequisites; then
        print_success "All required tools are installed"
    else
        validation_passed=false
    fi
    echo ""

    # Check GPG key
    print_info "Checking GPG key..."
    if get_gpg_key &> /dev/null; then
        local gpg_key
        gpg_key=$(get_gpg_key)
        print_success "GPG key found: $gpg_key"
    else
        print_error "No GPG key found"
        validation_passed=false
    fi
    echo ""

    # Check SOPS directory
    print_info "Checking SOPS directory..."
    if [ -d "$SOPS_DIR" ]; then
        print_success "SOPS directory exists: $SOPS_DIR"

        # Check permissions (Pop!_OS/Ubuntu 24.04 only)
        # NOTE: stat -c %a is Linux-specific
        local perms
        perms=$(stat -c %a "$SOPS_DIR" 2>/dev/null)
        if [ "$perms" = "700" ]; then
            print_success "Directory permissions are secure (700)"
        else
            print_warning "Directory permissions are $perms (should be 700)"
        fi
    else
        print_warning "SOPS directory does not exist: $SOPS_DIR"
    fi
    echo ""

    # Check SOPS configuration
    print_info "Checking SOPS configuration..."
    if [ -f "$SOPS_CONFIG" ]; then
        print_success "SOPS configuration exists: $SOPS_CONFIG"

        # Check if configuration is valid
        if grep -q "pgp:" "$SOPS_CONFIG"; then
            print_success "Configuration contains GPG key"
        else
            print_error "Configuration is missing GPG key"
            validation_passed=false
        fi
    else
        print_warning "SOPS configuration does not exist: $SOPS_CONFIG"
        validation_passed=false
    fi
    echo ""

    # Check secrets file
    print_info "Checking secrets file..."
    if [ -f "$SECRETS_FILE" ]; then
        print_success "Secrets file exists: $SECRETS_FILE"

        # Try to decrypt
        if sops --config "$SOPS_CONFIG" --decrypt "$SECRETS_FILE" &> /dev/null; then
            print_success "Secrets file can be decrypted"

            # Count secrets (excluding placeholders and comments)
            local secrets_count
            secrets_count=$(sops --config "$SOPS_CONFIG" --decrypt "$SECRETS_FILE" 2>/dev/null | \
                grep -v "^#" | \
                grep -v '_here$' | \
                grep "=" | \
                wc -l)

            if [ "$secrets_count" -gt 0 ]; then
                print_success "Found $secrets_count configured secret(s)"
            else
                print_warning "No secrets configured yet (all values are placeholders)"
            fi
        else
            print_error "Cannot decrypt secrets file"
            validation_passed=false
        fi
    else
        print_warning "Secrets file does not exist: $SECRETS_FILE"
        validation_passed=false
    fi
    echo ""

    # Summary
    if [ "$validation_passed" = true ]; then
        print_success "Validation passed! Your SOPS setup is ready to use."
    else
        print_error "Validation failed. Run: $0 init"
    fi
}

# Parse command line arguments
COMMAND="init"
SECRET_KEY=""

while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -y|--yes)
            YES_MODE=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        init|edit|view|validate|help)
            COMMAND=$1
            shift
            ;;
        add-secret)
            COMMAND=$1
            shift
            # Next argument should be the KEY name
            if [[ $# -gt 0 && ! "$1" =~ ^- ]]; then
                SECRET_KEY="$1"
                shift
            fi
            ;;
        *)
            print_error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

# Execute command
case "$COMMAND" in
    init)
        init_sops
        ;;
    edit)
        edit_secrets
        ;;
    view)
        view_secrets
        ;;
    add-secret)
        add_secret "$SECRET_KEY"
        ;;
    validate)
        validate_config
        ;;
    help)
        usage
        ;;
    *)
        print_error "Unknown command: $COMMAND"
        usage
        exit 1
        ;;
esac
