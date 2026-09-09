#!/usr/bin/env zsh
# 42-dev-ros.zsh - ROS (Robot Operating System) development environment
#
# This module sets up the ROS environment if available.
# It checks for standard ROS installation locations and sources the setup script.

# Only load in interactive shells
[[ ! -o interactive ]] && return 0

# Function to source ROS setup
load_ros_setup() {
    # List of ROS installation paths to check (in priority order)
    # Use setup.zsh for native zsh support
    local ros_setup_paths=(
        "/opt/ros/kilted/setup.zsh"
        "/opt/ros/humble/setup.zsh"
        "/opt/ros/noetic/setup.zsh"
        "/opt/ros/melodic/setup.zsh"
    )

    # Try each path until one succeeds
    for setup_path in "${ros_setup_paths[@]}"; do
        if [[ -f "$setup_path" ]]; then
            # Source the ROS setup file (native zsh version)
            # shellcheck disable=SC1090
            source "$setup_path" 2>/dev/null && return 0
        fi
    done

    # No ROS installation found - silent return
    return 1
}

# Function to manually reload ROS environment (for debugging)
reload_ros() {
    echo "=== Reloading ROS environment ==="

    # Check for ROS installation
    # Use setup.zsh for native zsh support
    local ros_setup_paths=(
        "/opt/ros/kilted/setup.zsh"
        "/opt/ros/humble/setup.zsh"
        "/opt/ros/noetic/setup.zsh"
        "/opt/ros/melodic/setup.zsh"
    )

    local found=0
    for setup_path in "${ros_setup_paths[@]}"; do
        if [[ -f "$setup_path" ]]; then
            echo "✓ Found ROS setup: $setup_path"
            # shellcheck disable=SC1090
            if source "$setup_path" 2>/dev/null; then
                echo "✓ ROS environment loaded successfully"
                [[ -n "${ROS_DISTRO:-}" ]] && echo "  ROS Distro: $ROS_DISTRO"
                [[ -n "${ROS_VERSION:-}" ]] && echo "  ROS Version: $ROS_VERSION"
                [[ -n "${ROS_PYTHON_VERSION:-}" ]] && echo "  Python Version: $ROS_PYTHON_VERSION"
                found=1
                return 0
            else
                echo "✗ Failed to source ROS setup: $setup_path"
            fi
        fi
    done

    if [[ $found -eq 0 ]]; then
        echo "✗ No ROS installation found"
        echo ""
        echo "  Checked paths:"
        for setup_path in "${ros_setup_paths[@]}"; do
            echo "    - $setup_path"
        done
        echo ""
        echo "  Install ROS first:"
        echo "    https://docs.ros.org/"
        return 1
    fi
}

# Function to check ROS environment status
ros_status() {
    echo "=== ROS Environment Status ==="
    echo ""

    # Check if ROS environment is loaded
    if [[ -n "${ROS_DISTRO:-}" ]]; then
        echo "Status: ✓ ROS environment loaded"
        echo ""
        echo "Environment Variables:"
        echo "  ROS_DISTRO: ${ROS_DISTRO}"
        [[ -n "${ROS_VERSION:-}" ]] && echo "  ROS_VERSION: ${ROS_VERSION}"
        [[ -n "${ROS_ROOT:-}" ]] && echo "  ROS_ROOT: ${ROS_ROOT}"
        [[ -n "${ROS_PACKAGE_PATH:-}" ]] && echo "  ROS_PACKAGE_PATH: ${ROS_PACKAGE_PATH}"
        [[ -n "${ROS_PYTHON_VERSION:-}" ]] && echo "  ROS_PYTHON_VERSION: ${ROS_PYTHON_VERSION}"
        [[ -n "${ROS_LOCALHOST_ONLY:-}" ]] && echo "  ROS_LOCALHOST_ONLY: ${ROS_LOCALHOST_ONLY}"

        echo ""
        echo "Commands:"
        command -v rosversion &>/dev/null && echo "  ✓ rosversion: $(rosversion -d 2>/dev/null || echo 'available')"
        command -v rospack &>/dev/null && echo "  ✓ rospack: available"
        command -v catkin &>/dev/null && echo "  ✓ catkin: available"
        command -v colcon &>/dev/null && echo "  ✓ colcon: available"
    else
        echo "Status: ✗ ROS environment not loaded"
        echo ""
        echo "Run: reload_ros"
        echo "Or check installation paths manually"
    fi
}

# Automatically load ROS environment on shell startup
load_ros_setup

# Note: If loading fails, it fails silently to avoid cluttering shell startup.
# Use 'reload_ros' or 'ros_status' to debug if needed.
