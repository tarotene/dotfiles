#!/bin/bash
set -euo pipefail

# SSH Permission Fix Script
# Ensures proper permissions for SSH directory and files

# Color codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# SSH directory
SSH_DIR="$HOME/.ssh"

echo "=== SSH Permission Fix ==="
echo ""

# Check if SSH directory exists
if [[ ! -d "$SSH_DIR" ]]; then
    echo -e "${YELLOW}SSH directory not found at $SSH_DIR${NC}"
    echo "Creating SSH directory..."
    mkdir -p "$SSH_DIR"
fi

# Fix SSH directory permissions (must be 700)
echo "Setting SSH directory permissions to 700..."
chmod 700 "$SSH_DIR"
echo -e "${GREEN}✓ SSH directory permissions set${NC}"

# Fix SSH file permissions
echo ""
echo "Fixing SSH file permissions..."

# Counter for fixed files
fixed_count=0

# Process all files in SSH directory
if [[ -d "$SSH_DIR" ]]; then
    while IFS= read -r file; do
        if [[ -f "$file" ]]; then
            current_perms=$(stat -c %a "$file" 2>/dev/null || stat -f %p "$file" | tail -c 4)
            filename=$(basename "$file")
            
            # Determine correct permissions
            if [[ "$filename" =~ \.pub$ ]]; then
                # Public keys can be 644 or 600
                if [[ "$current_perms" != "644" && "$current_perms" != "600" ]]; then
                    chmod 644 "$file"
                    echo -e "${GREEN}✓ Fixed $filename (public key) -> 644${NC}"
                    ((fixed_count++))
                fi
            elif [[ "$filename" == "config" ]]; then
                # SSH config should be 600
                if [[ "$current_perms" != "600" ]]; then
                    chmod 600 "$file"
                    echo -e "${GREEN}✓ Fixed $filename -> 600${NC}"
                    ((fixed_count++))
                fi
            elif [[ "$filename" == "authorized_keys" || "$filename" == "known_hosts" || "$filename" =~ known_hosts\..* ]]; then
                # These files should be 600
                if [[ "$current_perms" != "600" ]]; then
                    chmod 600 "$file"
                    echo -e "${GREEN}✓ Fixed $filename -> 600${NC}"
                    ((fixed_count++))
                fi
            elif [[ ! "$filename" =~ ^\. ]]; then
                # Any other non-hidden files (likely private keys) should be 600
                if [[ "$current_perms" != "600" ]]; then
                    chmod 600 "$file"
                    echo -e "${GREEN}✓ Fixed $filename -> 600${NC}"
                    ((fixed_count++))
                fi
            fi
        fi
    done < <(find "$SSH_DIR" -maxdepth 1 -type f 2>/dev/null)
fi

# Fix permissions for SSH config.d directory if it exists
if [[ -d "$SSH_DIR/config.d" ]]; then
    echo ""
    echo "Fixing SSH config.d directory..."
    chmod 700 "$SSH_DIR/config.d"
    
    # Fix all config files in config.d
    find "$SSH_DIR/config.d" -type f -name "*.conf" -exec chmod 600 {} \; 2>/dev/null || true
    echo -e "${GREEN}✓ Fixed config.d permissions${NC}"
fi

# Summary
echo ""
if [[ $fixed_count -eq 0 ]]; then
    echo -e "${GREEN}✓ All SSH file permissions are already correct!${NC}"
else
    echo -e "${GREEN}✓ Fixed permissions for $fixed_count file(s)${NC}"
fi

# Run security check if available
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -x "$SCRIPT_DIR/security-check.sh" ]]; then
    echo ""
    echo "Running security check for SSH..."
    "$SCRIPT_DIR/security-check.sh" | grep -A20 "Checking File Permissions" | grep -A10 "SSH" || true
fi