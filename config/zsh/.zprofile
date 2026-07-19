#!/usr/bin/env zsh
# Zsh profile - login shell initialization (computations and setup logic)

# Configuration variables for XDG Rust migration
RUST_XDG_AUTO_CLEANUP="${RUST_XDG_AUTO_CLEANUP:-1}"  # Set to 0 to disable automatic cleanup
RUST_XDG_LOG_FILE="${XDG_CACHE_HOME:-$HOME/.cache}/rust-xdg-migration.log"

# Function to log XDG migration operations
log_xdg_migration() {
    local message="$1"
    local timestamp="$(date '+%Y-%m-%d %H:%M:%S')"
    echo "[$timestamp] $message" >> "$RUST_XDG_LOG_FILE"
}

# Function to verify standard Rust installation safety
verify_standard_rust_installation() {
    log_xdg_migration "Verifying standard Rust installation safety"
    
    # Test 1: Check if .cargo/env exists
    if [[ ! -f "$HOME/.cargo/env" ]]; then
        log_xdg_migration "Safety check failed: ~/.cargo/env not found"
        return 1
    fi
    
    # Test 2: Check if cargo command is available
    if ! command -v cargo &>/dev/null; then
        log_xdg_migration "Safety check failed: cargo command not available"
        return 1
    fi
    
    # Test 3: Check if cargo can show version
    if ! cargo --version &>/dev/null; then
        log_xdg_migration "Safety check failed: cargo --version failed"
        return 1
    fi
    
    # Test 4: Check if rustc is available
    if ! command -v rustc &>/dev/null; then
        log_xdg_migration "Safety check failed: rustc command not available"
        return 1
    fi
    
    # Test 5: Check if rustc can show version
    if ! rustc --version &>/dev/null; then
        log_xdg_migration "Safety check failed: rustc --version failed"
        return 1
    fi
    
    log_xdg_migration "All standard Rust installation safety checks passed"
    return 0
}

# Function to create backup before cleanup
create_xdg_backup() {
    local xdg_cargo="$1"
    local xdg_rustup="$2"
    local backup_dir="${XDG_DATA_HOME:-$HOME/.local/share}/rust-xdg-backups/$(date +%Y%m%d_%H%M%S)"
    
    log_xdg_migration "Creating backup before XDG cleanup: $backup_dir"
    
    if ! mkdir -p "$backup_dir"; then
        log_xdg_migration "Error: Failed to create backup directory"
        return 1
    fi
    
    # Backup cargo if exists
    if [[ -d "$xdg_cargo" ]]; then
        if ! cp -r "$xdg_cargo" "$backup_dir/cargo"; then
            log_xdg_migration "Error: Failed to backup XDG cargo directory"
            return 1
        fi
        log_xdg_migration "Backed up XDG cargo to: $backup_dir/cargo"
    fi
    
    # Backup rustup if exists
    if [[ -d "$xdg_rustup" ]]; then
        if ! cp -r "$xdg_rustup" "$backup_dir/rustup"; then
            log_xdg_migration "Error: Failed to backup XDG rustup directory"
            return 1
        fi
        log_xdg_migration "Backed up XDG rustup to: $backup_dir/rustup"
    fi
    
    echo "$backup_dir"
    return 0
}

# Enhanced function to check for and handle existing XDG installations
_check_xdg_rust_migration() {
    local xdg_cargo="${XDG_DATA_HOME}/cargo"
    local xdg_rustup="${XDG_DATA_HOME}/rustup"
    
    log_xdg_migration "Starting XDG Rust migration check"
    
    if [[ -d "$xdg_cargo" ]] && [[ ! -d "$HOME/.cargo" ]]; then
        echo "⚠️  Rust tools found in XDG location: $xdg_cargo"
        echo "   Run 'migrate_rust_tools_from_xdg' to migrate to standard location"
        echo "   This will make your Rust tools accessible in PATH again"
        log_xdg_migration "XDG-only installation detected, migration recommended"
        return 0
    fi
    
    if [[ -d "$xdg_cargo" ]] && [[ -d "$HOME/.cargo" ]]; then
        log_xdg_migration "Both XDG and standard Rust installations detected"
        
        # Check if automatic cleanup is disabled
        if [ "$RUST_XDG_AUTO_CLEANUP" = "0" ]; then
            echo "ℹ️  Old XDG Rust installation found at: $xdg_cargo"
            echo "   Automatic cleanup disabled (RUST_XDG_AUTO_CLEANUP=0)"
            echo "   Run 'clean_old_xdg_rust' to remove it manually"
            log_xdg_migration "Automatic cleanup skipped (disabled by configuration)"
            return 0
        fi
        
        # Enhanced safety verification before cleanup
        if verify_standard_rust_installation; then
            log_xdg_migration "Standard installation verified, proceeding with automatic cleanup"
            
            # Create backup before cleanup
            local backup_dir
            if backup_dir="$(create_xdg_backup "$xdg_cargo" "$xdg_rustup")"; then
                echo "🧹 Cleaning up old XDG Rust installation at: $xdg_cargo"
                log_xdg_migration "Starting automatic cleanup with backup at: $backup_dir"
                
                # Safe cleanup with verification
                local cleanup_success=true
                if [[ -d "$xdg_cargo" ]]; then
                    if rm -rf "$xdg_cargo" 2>/dev/null; then
                        log_xdg_migration "Successfully removed XDG cargo directory"
                    else
                        log_xdg_migration "Error: Failed to remove XDG cargo directory"
                        cleanup_success=false
                    fi
                fi
                
                if [[ -d "$xdg_rustup" ]]; then
                    if rm -rf "$xdg_rustup" 2>/dev/null; then
                        log_xdg_migration "Successfully removed XDG rustup directory"
                    else
                        log_xdg_migration "Error: Failed to remove XDG rustup directory"
                        cleanup_success=false
                    fi
                fi
                
                if [ "$cleanup_success" = true ]; then
                    echo "   ✅ Cleanup complete - standard Rust installation confirmed working"
                    echo "   📁 Backup available at: $backup_dir"
                    log_xdg_migration "Cleanup completed successfully"
                else
                    echo "   ⚠️  Cleanup partially failed - check logs for details"
                    log_xdg_migration "Cleanup completed with errors"
                fi
            else
                echo "   ⚠️  Backup creation failed - skipping automatic cleanup for safety"
                log_xdg_migration "Automatic cleanup skipped due to backup failure"
            fi
        else
            echo "ℹ️  Old XDG Rust installation found at: $xdg_cargo"
            echo "   Standard installation verification failed - skipping automatic cleanup"
            echo "   Run 'clean_old_xdg_rust' to remove it manually after verification"
            log_xdg_migration "Automatic cleanup skipped due to safety verification failure"
        fi
        return 0
    fi
    
    log_xdg_migration "No XDG installations detected"
}

# Migration helper function
migrate_rust_tools_from_xdg() {
    local xdg_cargo="${XDG_DATA_HOME}/cargo"
    local xdg_rustup="${XDG_DATA_HOME}/rustup"
    
    echo "=== Rust Tools Migration from XDG ==="
    
    if [[ ! -d "$xdg_cargo" ]]; then
        echo "No XDG Cargo installation found at: $xdg_cargo"
        return 0
    fi
    
    if [[ -d "$HOME/.cargo" ]]; then
        echo "Standard Cargo directory already exists at: $HOME/.cargo"
        echo "Please manually resolve the conflict before migration"
        return 1
    fi
    
    echo "Migrating Cargo from $xdg_cargo to $HOME/.cargo"
    
    # Create backup
    local backup_dir="${XDG_DATA_HOME}/cargo-backup-$(date +%Y%m%d_%H%M%S)"
    echo "Creating backup at: $backup_dir"
    cp -r "$xdg_cargo" "$backup_dir"
    
    # Move to standard location
    mv "$xdg_cargo" "$HOME/.cargo"
    
    # Migrate rustup if it exists
    if [[ -d "$xdg_rustup" ]] && [[ ! -d "$HOME/.rustup" ]]; then
        echo "Migrating Rustup from $xdg_rustup to $HOME/.rustup"
        mv "$xdg_rustup" "$HOME/.rustup"
    fi
    
    echo "✅ Migration complete!"
    echo "   Cargo tools should now be accessible in PATH"
    echo "   Backup available at: $backup_dir"
    echo "   Please restart your shell or run: source ~/.zshenv"
}

# Enhanced cleanup helper function for old XDG installations
clean_old_xdg_rust() {
    local xdg_cargo="${XDG_DATA_HOME}/cargo"
    local xdg_rustup="${XDG_DATA_HOME}/rustup"
    
    log_xdg_migration "Manual cleanup function called"
    
    if [[ ! -d "$xdg_cargo" ]] && [[ ! -d "$xdg_rustup" ]]; then
        echo "ℹ️  No old XDG Rust installations found to clean up"
        log_xdg_migration "No XDG installations found for cleanup"
        return 0
    fi
    
    # Enhanced safety verification
    if ! verify_standard_rust_installation; then
        echo "⚠️  Standard Rust installation verification failed"
        echo "   Please ensure ~/.cargo/env exists and cargo is accessible"
        echo "   before cleaning up the old XDG installation"
        log_xdg_migration "Manual cleanup aborted due to safety verification failure"
        return 1
    fi
    
    echo "🧹 Cleaning up old XDG Rust installations..."
    log_xdg_migration "Starting manual cleanup process"
    
    # Create backup before cleanup
    local backup_dir
    if backup_dir="$(create_xdg_backup "$xdg_cargo" "$xdg_rustup")"; then
        echo "   📁 Backup created at: $backup_dir"
        log_xdg_migration "Backup created successfully for manual cleanup"
        
        # Safe cleanup with verification
        local cleanup_success=true
        if [[ -d "$xdg_cargo" ]]; then
            echo "   Removing: $xdg_cargo"
            if rm -rf "$xdg_cargo" 2>/dev/null; then
                log_xdg_migration "Successfully removed XDG cargo directory (manual)"
            else
                log_xdg_migration "Error: Failed to remove XDG cargo directory (manual)"
                cleanup_success=false
            fi
        fi
        
        if [[ -d "$xdg_rustup" ]]; then
            echo "   Removing: $xdg_rustup"
            if rm -rf "$xdg_rustup" 2>/dev/null; then
                log_xdg_migration "Successfully removed XDG rustup directory (manual)"
            else
                log_xdg_migration "Error: Failed to remove XDG rustup directory (manual)"
                cleanup_success=false
            fi
        fi
        
        if [ "$cleanup_success" = true ]; then
            echo "   ✅ Cleanup complete - old XDG installations removed"
            log_xdg_migration "Manual cleanup completed successfully"
        else
            echo "   ⚠️  Cleanup partially failed - check logs for details"
            log_xdg_migration "Manual cleanup completed with errors"
        fi
    else
        echo "   ⚠️  Backup creation failed - aborting cleanup for safety"
        log_xdg_migration "Manual cleanup aborted due to backup failure"
        return 1
    fi
}

# Function to test XDG migration functionality
test_xdg_migration_logic() {
    log_xdg_migration "Starting XDG migration logic tests"
    echo "🧪 Testing XDG migration functionality..."
    
    local test_failures=0
    
    # Test 1: Verify standard Rust installation detection
    echo "   Test 1: Standard Rust installation verification..."
    if verify_standard_rust_installation; then
        echo "   ✅ Standard Rust installation verification passed"
        log_xdg_migration "Test 1 passed: Standard Rust verification"
    else
        echo "   ❌ Standard Rust installation verification failed"
        log_xdg_migration "Test 1 failed: Standard Rust verification"
        test_failures=$((test_failures + 1))
    fi
    
    # Test 2: Log function availability
    echo "   Test 2: Logging function availability..."
    if command -v log_xdg_migration &>/dev/null; then
        echo "   ✅ Logging function available"
        log_xdg_migration "Test 2 passed: Logging function available"
    else
        echo "   ❌ Logging function not available"
        test_failures=$((test_failures + 1))
    fi
    
    # Test 3: Configuration variable detection
    echo "   Test 3: Configuration variables..."
    if [[ -n "$RUST_XDG_AUTO_CLEANUP" ]] && [[ -n "$RUST_XDG_LOG_FILE" ]]; then
        echo "   ✅ Configuration variables properly set"
        log_xdg_migration "Test 3 passed: Configuration variables set (AUTO_CLEANUP=$RUST_XDG_AUTO_CLEANUP, LOG_FILE=$RUST_XDG_LOG_FILE)"
    else
        echo "   ❌ Configuration variables not properly set"
        log_xdg_migration "Test 3 failed: Configuration variables missing"
        test_failures=$((test_failures + 1))
    fi
    
    # Test 4: Backup function availability
    echo "   Test 4: Backup function availability..."
    if command -v create_xdg_backup &>/dev/null; then
        echo "   ✅ Backup function available"
        log_xdg_migration "Test 4 passed: Backup function available"
    else
        echo "   ❌ Backup function not available"
        log_xdg_migration "Test 4 failed: Backup function not available"
        test_failures=$((test_failures + 1))
    fi
    
    # Test 5: Log file writability
    echo "   Test 5: Log file writability..."
    local test_message="Test message $(date +%s)"
    if log_xdg_migration "$test_message" && grep -q "$test_message" "$RUST_XDG_LOG_FILE" 2>/dev/null; then
        echo "   ✅ Log file is writable and functional"
        log_xdg_migration "Test 5 passed: Log file writability confirmed"
    else
        echo "   ❌ Log file is not writable"
        test_failures=$((test_failures + 1))
    fi
    
    # Summary
    if [ $test_failures -eq 0 ]; then
        echo "   🎉 All XDG migration tests passed!"
        log_xdg_migration "All XDG migration tests completed successfully"
        return 0
    else
        echo "   ⚠️  $test_failures test(s) failed - check logs for details"
        log_xdg_migration "XDG migration tests completed with $test_failures failures"
        return 1
    fi
}

# History file settings (permission issue handling)
setup_zsh_history() {
    export HISTFILE="${ZDOTDIR}/.zsh_history"
    if [[ ! -w "$(dirname "$HISTFILE")" ]]; then
        export HISTFILE="${XDG_CACHE_HOME}/zsh_history"
        # Final fallback
        [[ ! -w "$(dirname "$HISTFILE")" ]] && export HISTFILE="/tmp/.zsh_history"
    fi
}

# Environment verification function
check_dotfiles_env() {
    echo "=== Dotfiles Environment Check ==="
    echo "DOTFILES_DIR: $DOTFILES_DIR"
    echo "XDG_CONFIG_HOME: $XDG_CONFIG_HOME"
    echo "XDG_DATA_HOME: $XDG_DATA_HOME"
    echo "ZDOTDIR: $ZDOTDIR"
    echo "HISTFILE: $HISTFILE"
    echo "PATH includes cargo: "
    echo "$PATH" | tr ':' '\n' | grep cargo || echo "  NOT FOUND"
}

# === Login Shell Initialization ===

# Run migration check (only show warning once per session)
if [[ -z "$_RUST_MIGRATION_CHECKED" ]]; then
    _check_xdg_rust_migration
    export _RUST_MIGRATION_CHECKED=1
fi

# Setup history file with fallback handling
setup_zsh_history

# Load Cargo environment if available
if [[ -f "$HOME/.cargo/env" ]]; then
    source "$HOME/.cargo/env"
elif [[ -d "$HOME/.cargo/bin" ]]; then
    # Manual PATH addition (fallback)
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# CRITICAL: Load PATH configuration for login shells
# This ensures ~/.local/bin and other paths are available in login shells
# Normally these are loaded via .zshrc, but login shells don't source .zshrc
local _path_module="${ZDOTDIR:-$HOME/.config/zsh}/modules/10-path.zsh"
if [[ -r "$_path_module" ]]; then
    # Source with error handling to prevent breaking login shell initialization
    if ! source "$_path_module" 2>/dev/null; then
        # Fallback: manually add critical paths if module fails
        [[ -d "$HOME/.local/bin" ]] && export PATH="$HOME/.local/bin:$PATH"
        [[ -d "$HOME/bin" ]] && export PATH="$HOME/bin:$PATH"
    fi
elif [[ -f "$_path_module" ]]; then
    # File exists but not readable
    echo "Warning: Cannot read $_path_module (check permissions)" >&2
fi