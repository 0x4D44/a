# Alias Manager (`a`) v1.3.0

A cross-platform command alias management tool written in Rust. Provides a simple way to create, manage, and execute command aliases that work on both Windows and Linux.

## Features

- **Cross-platform**: Works on Windows, Linux, and macOS
- **Fast**: Rust implementation with quick startup times
- **Simple**: Single binary with intuitive command interface
- **Safe**: Reserved namespace prevents conflicts with management commands
- **Persistent**: Aliases are stored in JSON configuration file
- **Enhanced UX**: Beautiful colorized output with emojis and visual hierarchy
- **Interactive help**: Optional examples with user-friendly prompts
- **Overwrite protection**: Prevents accidental alias overwrites with confirmation prompts
- **Advanced command chaining**: Sophisticated workflow automation with multiple operators
- **Parallel execution**: Run multiple commands simultaneously with thread synchronization
- **Conditional logic**: Smart execution based on exit codes and success/failure states
- **Progress feedback**: Clear visibility into execution progress and command flow
- **Backward compatibility**: Seamless migration from older versions
- **Cross-platform security**: No shell dependency eliminates injection vulnerabilities
- **GitHub sync**: Push and pull config.json with a single command

## Command Chaining

The alias manager supports sophisticated command chaining with multiple operators for complex workflow automation. Commands can be chained with different conditional logic and execution modes.

### Operators Available:

**Sequential Operators (default execution mode):**
- **`--and`** (&&): Run if previous command succeeded (exit code 0)
- **`--or`** (||): Run if previous command failed (exit code ≠ 0)  
- **`--always`** (;): Always run regardless of previous command result
- **`--if-code <N>`** (?[N]): Run only if previous command exit code equals N
- **`--chain`** (legacy): Same as `--and` for backward compatibility

**Execution Modes:**
- **Sequential** (default): Commands run one after another with conditional logic
- **Parallel** (`--parallel`): All commands run simultaneously in separate threads

### How Sequential Execution Works:
- Commands execute **one at a time** in the order specified
- Each operator checks the **exit code** of the previous command
- **Progress feedback**: Shows which step is executing (e.g., `[2/4] (&&) Executing: npm test`)
- **Skip logic**: Commands that don't meet their condition are skipped with explanation
- **Additional arguments**: Passed only to the **last command** in the chain
- **Interrupt handling**: Ctrl+C stops current command and terminates the chain

### Examples:

```bash
# Simple sequential execution (equivalent to shell &&)
a --add deploy "npm run build" --and "npm test" --and "npm run deploy"

# Complex conditional logic - run deploy if tests pass, or show error if they fail
a --add smart "npm test" --and "npm run deploy" --or "echo 'Tests failed!'"

# Exit code handling - different actions based on specific exit codes
a --add check "npm test" --if-code 0 "echo 'All good!'" --if-code 1 "echo 'Tests failed'"

# Always run cleanup regardless of success/failure
a --add build "npm run build" --and "npm run deploy" --always "npm run cleanup"

# Parallel execution - all commands run simultaneously
a --add lint "npm run lint" --and "npm run test" --and "npm run typecheck" --parallel

# Mixed operators in complex workflow
a --add deploy "npm run build" --and "npm test" --and "npm run deploy" --or "npm run rollback" --always "npm run notify"
```

### Execution Output Examples:

**Sequential with skipping:**
```bash
a deploy  # If tests fail
[1/4] Executing: npm run build
[2/4] (&&) Executing: npm test  
[3/4] Skipping: npm run deploy (previous command failed)
[4/4] (;) Executing: npm run cleanup
```

**Parallel execution:**
```bash
a lint
Executing 3 commands in parallel
Started: npm run lint
Started: npm run test  
Started: npm run typecheck
Completed [1]: exit code 0
Completed [2]: exit code 0
Completed [3]: exit code 0
All parallel commands completed successfully
```

### Design Benefits:
- **Cross-platform**: No shell dependency, works identically on Windows/Linux/macOS
- **Predictable**: Each command executes exactly as specified, no shell interpretation
- **Secure**: No shell injection vulnerabilities  
- **Clear feedback**: User sees exactly what's running, what's skipped, and why
- **Flexible**: Mix and match operators for complex workflow logic
- **Robust error handling**: Failed commands don't crash the chain, just affect flow
- **Thread-safe**: Parallel execution uses proper synchronization

### Advanced Use Cases:

```bash
# Deployment with rollback capability
a --add deploy "npm run build" --and "npm run test" --and "deploy.sh" --or "rollback.sh" --always "cleanup.sh"

# Development workflow with multiple checks
a --add check "npm run lint" --and "npm run test" --and "npm run audit" --or "npm run fix" --parallel

# Exit-code specific handling  
a --add git-check "git status --porcelain" --if-code 0 "echo 'Clean!'" --if-code 1 "git add . && git commit -m 'Auto commit'"

# Complex parallel testing
a --add test-all "npm run unit-tests" --and "npm run integration-tests" --and "npm run e2e-tests" --parallel
```

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

# Export local config to current directory
a --export

# Push/pull config with GitHub
a --push                       # requires A_GITHUB_TOKEN / GITHUB_TOKEN / GH_TOKEN
a --pull                       # pulls latest (backs up existing)
```

## Configuration

Aliases are stored in:
- **Windows**: `%USERPROFILE%\.alias-mgr\config.json`
- **Linux/macOS**: `~/.alias-mgr/config.json`

The configuration file is automatically created and updated when you add/remove aliases.

### Sync With GitHub

Repo information is hardcoded:
- Repo: `0x4d44/a`
- Branch: `main`
- Path: `config.json`

Requirements:
- Set an environment variable `A_GITHUB_TOKEN` (or `GITHUB_TOKEN`/`GH_TOKEN`) with repo access.

Usage:
```bash
# Push local config (~/.alias-mgr/config.json) to GitHub root as config.json
a --push                       # optional: --message "update aliases"

# Pull latest config from GitHub and overwrite local one (backs up to config.backup.json)
a --pull
```

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

- **v1.3.0**:
  - Add `--push` and `--pull` to sync config with GitHub via API
  - Defaults to repo `0x4d44/a`, branch `main`, path `config.json`
  - Uses `A_GITHUB_TOKEN`/`GITHUB_TOKEN`/`GH_TOKEN` for authentication (push requires token)
  - Safe pull: creates `config.backup.json` before overwriting

- **v1.1.0**:
  - **Enhanced Help System**: Interactive help with optional examples
  - **Improved UX**: Added emojis and enhanced color coding throughout interface
  - **Better Navigation**: Examples are now optional (prompted) to reduce information overload
  - **Visual Polish**: More colorful and organized help display
  - **User-Friendly**: Pause before examples with Y/n prompt (Enter defaults to yes)

- **v1.0.0**: 
  - Added colorized output, single-line list format, version display, config location command
  - Improved overwrite protection with proper "Added" vs "Updated" messaging
  - **MAJOR ENHANCEMENT**: Advanced command chaining with multiple operators:
    - **`--and`** (&&): Run if previous succeeded
    - **`--or`** (||): Run if previous failed
    - **`--always`** (;): Always run regardless
    - **`--if-code <N>`**: Run if previous exit code equals N
    - **`--parallel`**: Execute all commands simultaneously
  - Enhanced execution engine with sophisticated conditional logic
  - Added parallel execution support with thread synchronization  
  - Comprehensive progress feedback and skip notifications
  - Backward compatibility with legacy `--chain` commands
  - Added config file migration for seamless upgrades
  - Fixed critical chaining bug with proper sequential execution
  - Enhanced error handling with graceful failure modes
