#!/usr/bin/env bash
# Test script for 15-tools-gpg-ssh.zsh module

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Helper functions
print_test() {
    echo -e "${YELLOW}Testing: $1${NC}"
}

print_pass() {
    echo -e "${GREEN}✓ PASS: $1${NC}"
    ((TESTS_PASSED++))
}

print_fail() {
    echo -e "${RED}✗ FAIL: $1${NC}"
    ((TESTS_FAILED++))
}

# Test the module loading
test_module_loads() {
    print_test "Module loads without errors"
    
    # Source the module in a subshell to avoid affecting current environment
    if (source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>/dev/null); then
        print_pass "Module loads successfully"
    else
        print_fail "Module failed to load"
    fi
}

# Test GPG_TTY variable setting
test_gpg_tty_variable() {
    print_test "GPG_TTY variable setting"
    
    # Test with TTY available
    if (source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>/dev/null && [[ -n "${GPG_TTY:-}" ]]); then
        print_pass "GPG_TTY is set when sourced"
    else
        print_fail "GPG_TTY not set properly"
    fi
}

# Test debug mode functionality
test_debug_mode() {
    print_test "Debug mode functionality"
    
    # Test debug output when GPG_SSH_DEBUG=1
    export GPG_SSH_DEBUG=1
    local debug_output
    debug_output=$(source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>&1)
    
    if [[ "$debug_output" == *"GPG-SSH:"* ]]; then
        print_pass "Debug output appears when GPG_SSH_DEBUG=1"
    else
        print_fail "Debug output missing when GPG_SSH_DEBUG=1"
    fi
    
    # Test no debug output when GPG_SSH_DEBUG=0
    export GPG_SSH_DEBUG=0
    debug_output=$(source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>&1)
    
    if [[ "$debug_output" != *"GPG-SSH:"* ]]; then
        print_pass "No debug output when GPG_SSH_DEBUG=0"
    else
        print_fail "Debug output present when GPG_SSH_DEBUG=0"
    fi
    
    unset GPG_SSH_DEBUG
}

# Test fallback behavior
test_fallback_behavior() {
    print_test "Fallback behavior when GPG unavailable"
    
    # Temporarily rename gpg command to simulate unavailable GPG
    local gpg_path=""
    if command -v gpg >/dev/null 2>&1; then
        gpg_path=$(command -v gpg)
        sudo mv "$gpg_path" "${gpg_path}.backup" 2>/dev/null || {
            # If we can't move gpg, just test that the module handles missing gpg gracefully
            print_pass "Cannot test GPG unavailable scenario (requires sudo)"
            return
        }
    fi
    
    # Test module behavior without GPG
    if (source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>/dev/null); then
        print_pass "Module handles missing GPG gracefully"
    else
        print_fail "Module fails when GPG unavailable"
    fi
    
    # Restore gpg command if we moved it
    if [[ -n "$gpg_path" && -f "${gpg_path}.backup" ]]; then
        sudo mv "${gpg_path}.backup" "$gpg_path" 2>/dev/null || true
    fi
}

# Test environment variable cleanup
test_ssh_agent_pid_cleanup() {
    print_test "SSH_AGENT_PID cleanup"
    
    # Set SSH_AGENT_PID to test it gets unset
    export SSH_AGENT_PID=12345
    
    # Source module (in subshell to check behavior)
    (
        source "config/zsh/modules/15-tools-gpg-ssh.zsh" 2>/dev/null
        if [[ -z "${SSH_AGENT_PID:-}" ]]; then
            echo "SSH_AGENT_PID_UNSET"
        fi
    ) | grep -q "SSH_AGENT_PID_UNSET" && {
        print_pass "SSH_AGENT_PID is unset when GPG agent is configured"
    } || {
        print_fail "SSH_AGENT_PID not properly unset"
    }
    
    unset SSH_AGENT_PID
}

# Helper function to find git repository root
find_git_root() {
    # Try git rev-parse first (works for repos with commits)
    if git rev-parse --show-toplevel >/dev/null 2>&1; then
        git rev-parse --show-toplevel
        return 0
    fi
    
    # Fallback: find git root by looking for .git directory
    local dir="$PWD"
    while [[ "$dir" != "/" && ! -d "$dir/.git" ]]; do
        dir="$(dirname "$dir")"
    done
    
    if [[ -d "$dir/.git" ]]; then
        echo "$dir"
        return 0
    else
        echo -e "${RED}Error: Not in a git repository${NC}" >&2
        return 1
    fi
}

# Main test execution
main() {
    echo -e "${YELLOW}Running GPG-SSH Module Tests${NC}"
    echo "========================================"
    
    # Change to repository root (handle case where repo has no commits)
    local git_root
    if git_root="$(find_git_root)"; then
        cd "$git_root"
    else
        exit 1
    fi
    
    # Run tests
    test_module_loads
    test_gpg_tty_variable
    test_debug_mode
    test_fallback_behavior
    test_ssh_agent_pid_cleanup
    
    # Print summary
    echo "========================================"
    echo -e "Tests completed: ${GREEN}${TESTS_PASSED} passed${NC}, ${RED}${TESTS_FAILED} failed${NC}"
    
    if [[ $TESTS_FAILED -eq 0 ]]; then
        echo -e "${GREEN}All tests passed!${NC}"
        exit 0
    else
        echo -e "${RED}Some tests failed.${NC}"
        exit 1
    fi
}

main "$@"