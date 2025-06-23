# Alias Manager (`a`) v1.0.0

A cross-platform command alias management tool written in Rust. Provides a simple way to create, manage, and execute command aliases that work on both Windows and Linux.

## Features

- **Cross-platform**: Works on Windows, Linux, and macOS
- **Fast**: Rust implementation with quick startup times
- **Simple**: Single binary with intuitive command interface
- **Safe**: Reserved namespace prevents conflicts with management commands
- **Persistent**: Aliases are stored in JSON configuration file
- **Colorized output**: Easy-to-read colored terminal output
- **Overwrite protection**: Prevents accidental alias overwrites with confirmation prompts
- **Command chaining**: Chain multiple commands together with `&&` operator

## Command Chaining

The `--chain` option allows you to chain multiple commands together using the `&&` operator. This means subsequent commands only run if the previous command succeeds.

### Examples:
```bash
# Build, test, and deploy (each step only runs if previous succeeds)
a --add deploy "npm run build" --chain "npm test" --chain "npm run deploy"

# Create project structure
a --add newproj "mkdir myproject" --chain "cd myproject" --chain "git init"

# Multiple chains can be mixed with other options
a --add fullbuild "cargo build" --chain "cargo test" --desc "Build and test" --force
```

### How it works:
- Each `--chain` command is appended with ` && `
- Commands are executed sequentially
- If any command fails (non-zero exit code), the chain stops
- You can use `--chain` multiple times in a single command

## Installation

### Build from Source

```bash
cd C:\language\a  # or your preferred directory
cargo build --release
```

The binary will be created at `target/release/a.exe` (Windows) or `target/release/a` (Linux/macOS).

### Add to PATH

Copy the binary to a directory in your PATH, or add the target directory to your PATH:

**Windows:**
```cmd
copy target\release\a.exe C:\Windows\System32\a.exe
```

**Linux/macOS:**
```bash
cp target/release/a /usr/local/bin/a
```

## Usage

### Management Commands (--prefix)

```bash
# Add a new alias
a --add <n> <command> [--desc "description"] [--force] [--chain <command2>]

# List all aliases (or filter)
a --list [filter]

# Remove an alias
a --remove <n>

# Show what an alias does
a --which <n>

# Show config file location
a --config

# Show version information
a --version

# Show help
a --help
```

### Executing Aliases

```bash
# Execute an alias
a <alias_name> [arguments...]
```

## Examples

```bash
# Create aliases
a --add gst "git status" --desc "Quick git status"
a --add glog "git log --oneline -10" --desc "Recent commits"
a --add deploy "docker-compose up -d && kubectl apply -f k8s/"

# Create chained aliases
a --add build "npm run build" --chain "npm test" --chain "npm run deploy"
a --add setup "mkdir project" --chain "cd project" --chain "npm init -y"

# List aliases
a --list                 # Show all (colorized, single-line format)
a --list git            # Show aliases containing 'git'

# Execute aliases
a gst                   # Runs: git status
a glog                  # Runs: git log --oneline -10
a deploy                # Runs: docker-compose up -d && kubectl apply -f k8s/
a build                 # Runs: npm run build && npm test && npm run deploy

# Pass arguments to aliases
a glog --graph          # Runs: git log --oneline -10 --graph

# Get info about aliases
a --which gst           # Shows what 'gst' executes

# Remove aliases
a --remove deploy       # Removes the deploy alias

# Force overwrite without confirmation
a --add gst "git status --short" --force

# Show where config is stored
a --config

# Show version
a --version
```

## Configuration

Aliases are stored in:
- **Windows**: `%USERPROFILE%\.alias-mgr\config.json`
- **Linux/macOS**: `~/.alias-mgr/config.json`

The configuration file is automatically created and updated when you add/remove aliases.

### Example Configuration

```json
{
  "aliases": {
    "gst": {
      "command": "git status",
      "description": "Quick git status",
      "created": "2025-06-23"
    },
    "deploy": {
      "command": "docker-compose up -d",
      "description": null,
      "created": "2025-06-23"
    }
  }
}
```

## Color Output

The tool uses ANSI color codes for enhanced readability:
- **Green**: Alias names and success messages
- **Blue**: Commands
- **Cyan**: Headers and field labels
- **Yellow**: Warnings and prompts
- **Gray**: Descriptions, dates, and secondary information

## Safety Features

### Overwrite Protection
- Automatically detects when you're trying to redefine an existing alias
- Shows current vs. new command for comparison
- Prompts for confirmation before overwriting
- Use `--force` flag to bypass confirmation

### Reserved Names
The following alias names are reserved and cannot be used:
- Any name starting with `--` (e.g., `--add`, `--list`)
- Any name containing `mgr:` 
- Any name starting with `.`

## Error Handling

- If an alias doesn't exist, you'll get a clear error message
- If a command fails, the tool will exit with the same error code
- Invalid alias names are rejected with helpful error messages
- All error messages are colorized for better visibility

## Development

### Running Tests

```bash
cargo test
```

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

## Architecture

The tool uses a dispatcher pattern where:
1. Management commands use the `--` prefix namespace
2. Alias execution uses bare names
3. All data is stored in a single JSON configuration file
4. Cross-platform file paths are handled automatically
5. ANSI color codes provide visual feedback

This design eliminates conflicts between management commands and user aliases while providing a clean, intuitive interface with enhanced visual feedback.

## Version History

- **v1.0.0**: 
  - Added colorized output, single-line list format, version display, config location command
  - Improved overwrite protection with proper "Added" vs "Updated" messaging
  - Added command chaining support with `--chain` option
  - Fixed messaging bug where new aliases incorrectly showed "Updated"
