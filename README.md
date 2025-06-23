# Alias Manager (`a`)

A cross-platform command alias management tool written in Rust. Provides a simple way to create, manage, and execute command aliases that work on both Windows and Linux.

## Features

- **Cross-platform**: Works on Windows, Linux, and macOS
- **Fast**: Rust implementation with quick startup times
- **Simple**: Single binary with intuitive command interface
- **Safe**: Reserved namespace prevents conflicts with management commands
- **Persistent**: Aliases are stored in JSON configuration file

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
a --add <name> <command> [--desc "description"]

# List all aliases (or filter)
a --list [filter]

# Remove an alias
a --remove <name>

# Show what an alias does
a --which <name>

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

# List aliases
a --list                 # Show all
a --list git            # Show aliases containing 'git'

# Execute aliases
a gst                   # Runs: git status
a glog                  # Runs: git log --oneline -10
a deploy                # Runs: docker-compose up -d && kubectl apply -f k8s/

# Pass arguments to aliases
a glog --graph          # Runs: git log --oneline -10 --graph

# Get info about aliases
a --which gst           # Shows what 'gst' executes

# Remove aliases
a --remove deploy       # Removes the deploy alias
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

## Reserved Names

The following alias names are reserved and cannot be used:
- Any name starting with `--` (e.g., `--add`, `--list`)
- Any name containing `mgr:` 
- Any name starting with `.`

## Error Handling

- If an alias doesn't exist, you'll get a clear error message
- If a command fails, the tool will exit with the same error code
- Invalid alias names are rejected with helpful error messages

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

This design eliminates conflicts between management commands and user aliases while providing a clean, intuitive interface.
