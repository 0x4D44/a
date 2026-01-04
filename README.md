# Alias Manager (`a`) v1.5.0

A cross-platform command alias management tool written in Rust. Provides a powerful and intuitive way to create, manage, and execute command aliases with advanced features like command chaining, parallel execution, conditional logic, and parameter substitution that work seamlessly across Windows, Linux, and macOS.

## Features

- **Cross-platform**: Works on Windows, Linux, and macOS
- **Fast**: Rust implementation with quick startup times
- **Simple**: Single binary with intuitive command interface
- **Safe**: Reserved namespace prevents conflicts with management commands
- **Persistent**: Aliases are stored in JSON configuration file
- **Enhanced UX**: Beautiful colorized output with emojis and visual hierarchy
- **Interactive help**: Use `--help --examples` for guided walkthroughs without prompts
- **Overwrite protection**: Prevents accidental alias overwrites with confirmation prompts
- **Advanced command chaining**: Sophisticated workflow automation with multiple operators
- **Parallel execution**: Run multiple commands simultaneously with thread synchronization
- **Conditional logic**: Smart execution based on exit codes and success/failure states
- **Progress feedback**: Clear visibility into execution progress and command flow
- **Backward compatibility**: Seamless migration from older versions
- **Cross-platform security**: No shell dependency eliminates injection vulnerabilities
- **GitHub sync**: Push and pull config.json with a single command (supports multiple auth methods)
- **Parameter substitution**: Use $1, $2, $@, $* for dynamic arguments in aliases
- **Smart argument handling**: Arguments passed intelligently to final command or substituted throughout chains

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

## Parameter Substitution

Aliases support dynamic parameter substitution, allowing you to create flexible, reusable commands that accept arguments at runtime.

### Substitution Syntax:

- **`$1, $2, $3, ...`**: Individual positional arguments (1-indexed, supports multi-digit like $10, $11)
- **`$@`**: All arguments as space-separated values
- **`$*`**: All arguments as space-separated values (equivalent to $@)
- **`$$`**: Literal dollar sign (escape sequence)

### How It Works:

When you use parameter variables in an alias command:
1. Arguments are **substituted** into the command string at the variable positions
2. The resolved command is then executed
3. If no variables are present, arguments are **appended** to the command (backward compatibility)

### Examples:

```bash
# Single parameter substitution
a --add tag-push "git tag $1" --and "git push origin $1"
a tag-push v1.2.3  # Executes: git tag v1.2.3 && git push origin v1.2.3

# Multiple parameters
a --add docker-deploy "docker tag $1:$2" --and "docker push $1:$2"
a docker-deploy myapp latest  # Executes: docker tag myapp:latest && docker push myapp:latest

# All arguments with $@
a --add test-files "pytest $@"
a test-files test1.py test2.py test3.py  # Executes: pytest test1.py test2.py test3.py

# Multiple occurrences of same parameter
a --add git-commit "git add $1" --and "git commit -m 'Updated $1'"
a git-commit README.md  # Executes: git add README.md && git commit -m 'Updated README.md'

# Literal dollar sign
a --add show-price "echo The price is $$100"
a show-price  # Executes: echo The price is $100

# Complex example with multiple parameters
a --add deploy "echo Deploying $1 to $2" --and "kubectl apply -f $1" --and "kubectl rollout status deployment/$1 -n $2"
a deploy myapp production  # Expands all $1 and $2 throughout the chain
```

### Behavior Notes:

- **With variables**: Arguments are substituted into the command template
- **Without variables**: Arguments are appended to the final command (legacy behavior)
- **Chain behavior**: If any command in a chain has parameter variables, arguments are available to all commands in the chain
- **Out-of-bounds**: Using `$5` when only 3 arguments are provided results in empty string substitution
- **Use `--which <alias>`**: To see how your parameters will be substituted with example values

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
a --help [--examples]
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
a --push                       # uses env/gh/git creds (see below)
a --pull                       # pulls latest (backs up existing)
```

## Configuration

Aliases are stored in:
- **Windows**: `%USERPROFILE%\.alias-mgr\config.json`
- **Linux/macOS**: `~/.alias-mgr/config.json`

## Testing & Coverage

Run the standard checks before pushing changes:

```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```

**WSL Note:** On WSL with Windows Git credential helper configured, set `GIT_TERMINAL_PROMPT=0` to prevent interactive prompts during tests that check GitHub auth behavior:

```bash
export GIT_TERMINAL_PROMPT=0  # add to ~/.bashrc for persistence
```

To inspect coverage:

```bash
# Fast JSON summary (captured by CI)
cargo llvm-cov --json --summary-only --output-path coverage-summary.json

# Optional deeper dives
cargo llvm-cov --text --show-missing-lines
cargo llvm-cov --html --open
```

We aim to keep **line coverage ≥ 80 %** and **function coverage ≥ 75 %**. Commit the refreshed `coverage-summary.json` alongside feature work so trends remain visible.

The configuration file is automatically created and updated when you add/remove aliases.

### Sync With GitHub

Repo information is hardcoded:
- Repo: `0x4d44/a`
- Branch: `main`
- Path: `config.json`

Auth sources (checked in order):
- Environment: `A_GITHUB_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`
- GitHub CLI: `gh auth status --show-token` or `gh auth token` (non-interactive)
- Git credential helper: token stored for `https://github.com` (used as password)

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

### Design Pattern

The tool uses a **dispatcher pattern** with distinct namespaces:
1. **Management commands**: Use the `--` prefix namespace (e.g., `--add`, `--list`, `--remove`)
2. **Alias execution**: Use bare names (user-defined aliases)
3. **Reserved names**: Protection against conflicts (no `--` prefix, `mgr:` substring, or `.` prefix)

### Core Components

**Data Structures:**
- `CommandType::Simple`: Single command (backward compatibility)
- `CommandType::Chain`: Complex command chains with conditional operators
- `ChainOperator`: Defines execution conditions (And, Or, Always, IfCode)
- `CommandChain`: Contains command sequences with optional parallel execution

**Execution Engine:**
- **Sequential mode**: Commands run one-by-one with conditional logic based on exit codes
- **Parallel mode**: All commands run simultaneously using `std::thread` and `mpsc::channel`
- **Smart argument handling**: Detects parameter variables and chooses appropriate argument passing strategy
- **Exit code tracking**: Maintains state across chain execution for conditional branching

**Configuration Management:**
- Single JSON file: `~/.alias-mgr/config.json` (cross-platform)
- Automatic serialization/deserialization with `serde`
- **Legacy migration**: Automatically converts old format configs
- **Atomic operations**: Safe concurrent access with proper file handling

### Cross-Platform Features

**Windows-Specific:**
- **PATHEXT resolution**: Automatically resolves executables using Windows PATHEXT environment variable
- **Extension inference**: Finds `.exe`, `.bat`, `.cmd`, `.com` files without explicit extension
- **Path search**: Searches directories in PATH for executable files

**Universal:**
- **No shell dependency**: Commands executed directly via `std::process::Command`
- **Security**: Eliminates shell injection vulnerabilities
- **Consistent behavior**: Same execution semantics across all platforms
- **Cross-platform paths**: Handles Windows (`%USERPROFILE%`) and Unix (`$HOME`) automatically

### Testing Infrastructure

- **Mock command runner**: Testable execution without running real commands
- **Mock GitHub client**: Isolated testing of GitHub sync functionality
- **Temporary file system**: Tests use `tempfile` crate for isolation
- **Environment guards**: Safe parallel test execution with environment variable protection
- **Comprehensive coverage**: Unit tests for all core functionality

This architecture provides a clean separation of concerns, robust error handling, and consistent behavior across all supported platforms.

## Dependencies

The tool uses carefully selected Rust crates for specific functionality:

- **`serde` & `serde_json`**: Configuration serialization/deserialization
- **`chrono`**: Timestamp management for alias creation dates
- **`ureq`**: Lightweight HTTP client for GitHub API interactions
- **`base64`**: Content encoding for GitHub file uploads
- **`shell-words`**: Proper shell-style command parsing (handles quotes, escaping)

**Development Dependencies:**
- **`tempfile`**: Isolated filesystem testing
- **`assert_cmd`, `assert_fs`, `predicates`**: Integration testing utilities

All dependencies are minimal and focused, avoiding heavy frameworks to maintain fast startup times and small binary size.

## Key Implementation Details

### Command Parsing

The tool uses `shell-words` crate to parse command strings, which properly handles:
- Quoted strings (single and double quotes)
- Escaped characters
- Whitespace handling
- Cross-platform compatibility

This ensures commands like `a --add test "echo \"hello world\""` work correctly.

### Parameter Substitution Algorithm

The parameter substitution engine:
1. Scans command strings for `$` followed by special characters
2. Supports multi-digit parameter indices (e.g., `$10`, `$11`)
3. Handles escape sequences (`$$` → `$`)
4. Processes `$@` and `$*` for all-arguments expansion
5. Preserves non-variable `$` characters as literals

### Windows Executable Resolution

On Windows, the tool implements sophisticated executable resolution:
1. Checks if program already has an extension
2. Reads `PATHEXT` environment variable (defaults to `.COM;.EXE;.BAT;.CMD`)
3. If path contains separators, searches in that directory first
4. Otherwise, searches all PATH directories with all PATHEXT extensions
5. Returns first match found

This mimics Windows shell behavior, allowing `npm` to resolve to `npm.cmd` automatically.

### GitHub Authentication Chain

Authentication for `--push` and `--pull` tries multiple sources in order:
1. Environment variables: `A_GITHUB_TOKEN`, `GITHUB_TOKEN`, `GH_TOKEN`
2. GitHub CLI: Runs `gh auth status --show-token` or `gh auth token` (non-interactive)
3. Git credentials: Queries `git credential fill` for stored tokens

This flexible approach works in various development environments (local, CI/CD, containers) without requiring specific setup.

### Thread-Safe Parallel Execution

Parallel command execution uses:
- **`Arc<dyn CommandRunner>`**: Shared command execution interface
- **`mpsc::channel`**: Thread communication for result collection
- **`thread::spawn`**: Separate threads for each command
- **Graceful aggregation**: Collects all results before reporting success/failure

This ensures safe concurrent execution with proper error handling.

## Version History

- **v1.5.0**:
  - Fix code formatting and clippy lints for CI compliance

- **v1.4.0**:
  - Enhance GitHub auth: support gh CLI and git credential helper in addition to env vars
  - Improved error message guidance for `a --push`

- **v1.3.0**:
  - Add `--push` and `--pull` to sync config with GitHub via API
  - Defaults to repo `0x4d44/a`, branch `main`, path `config.json`
  - Uses `A_GITHUB_TOKEN`/`GITHUB_TOKEN`/`GH_TOKEN` for authentication (push requires token)
  - Safe pull: creates `config.backup.json` before overwriting
  - Help examples moved behind `--help --examples` flag to keep default output non-interactive

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
