# Copilot Instructions for dotfiles

## Code Review Focus Areas

### Shell Scripts and Automation
- **Security**: Check for potential command injection, unsafe variable expansion, and proper quoting
- **Portability**: Ensure compatibility across different Unix-like systems (Linux, macOS, WSL)
- **Error Handling**: Verify proper error checking and graceful failure modes
- **Documentation**: Ensure clear usage examples and help documentation

### Configuration Files
- **Syntax Validation**: Check YAML, JSON, TOML, and other config file syntax
- **Security**: Look for hardcoded secrets or sensitive information
- **Consistency**: Ensure configuration follows established patterns
- **Documentation**: Verify inline comments explain complex configurations

### Package Management
- **Dependencies**: Review package lists for unnecessary or outdated packages
- **Cross-Platform**: Ensure package availability across supported platforms
- **Security**: Check for packages with known vulnerabilities
- **Organization**: Maintain clear categorization and documentation

### GitHub Workflows and Automation
- **Security**: Review permissions and secret handling
- **Efficiency**: Optimize for fast execution and resource usage
- **Reliability**: Ensure proper error handling and retry mechanisms
- **Maintainability**: Clear workflow structure and documentation

## Project-Specific Guidelines

### dotfiles Conventions
- **XDG Compliance**: Prefer XDG Base Directory specification
- **Modular Design**: Maintain separation of concerns
- **Cross-Platform Support**: Support Pop!_OS 22.04 and WSL Ubuntu 22.04
- **Dry-Run First**: Always provide safe testing modes
- **Comprehensive Logging**: Log operations to `local/*.log` files

### Code Quality Standards
- **Shell Scripts**: Use shellcheck-compliant bash with proper error handling
- **Documentation**: Maintain clear README files and inline documentation
- **Testing**: Include dry-run modes and safe operation verification
- **Backup Strategy**: Implement rollback mechanisms for system changes

## Review Priorities

1. **Security**: Always highest priority - prevent credential exposure and injection attacks
2. **Reliability**: Ensure scripts work consistently across environments
3. **Maintainability**: Code should be clear and well-documented
4. **Performance**: Optimize for reasonable execution times
5. **User Experience**: Provide clear feedback and error messages