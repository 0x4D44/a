# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

This is a Rust-based cross-platform command alias management tool called "Alias Manager" (`a`). It provides sophisticated command chaining with conditional logic, parallel execution, and persistent storage of aliases in JSON configuration files.

## Build and Development Commands

```bash
# Build and run
cargo build                    # Debug build
cargo build --release         # Optimized release build
cargo run                     # Run in development mode
cargo run -- --help           # Run with help flag

# Testing and quality
cargo test                     # Run all unit tests
cargo clippy                   # Run linter for code quality
cargo fmt                     # Format code according to Rust standards

# Installation
cargo build --release         # Build optimized binary
# Binary will be at target/release/a.exe (Windows) or target/release/a (Unix)
```

## Architecture Overview

The tool uses a **dispatcher pattern** with distinct namespaces:
- **Management commands**: Use `--` prefix (e.g., `--add`, `--list`, `--remove`)  
- **Alias execution**: Use bare names (user-defined aliases)
- **Configuration**: Single JSON file stored cross-platform in `~/.alias-mgr/config.json`

### Core Data Structures

**Command Types**:
- `CommandType::Simple(String)`: Single command (backward compatibility)
- `CommandType::Chain(CommandChain)`: Complex command chains with operators

**Command Chaining System**:
- `ChainOperator`: Enum defining conditional execution (`And`, `Or`, `Always`, `IfCode(i32)`)
- `CommandChain`: Contains vector of commands with operators and parallel execution flag
- **Sequential execution**: Commands run one-by-one with conditional logic based on exit codes
- **Parallel execution**: All commands run simultaneously with thread synchronization

**Configuration Management**:
- `Config`: Holds HashMap of aliases with automatic JSON serialization/deserialization
- **Legacy migration**: Automatically converts old format configs to new structure
- **Cross-platform paths**: Handles Windows (`%USERPROFILE%`) and Unix (`$HOME`) differences

### Execution Engine

**Sequential Chain Logic**:
- Tracks `last_exit_code` to determine if subsequent commands should execute
- Provides detailed progress feedback (`[2/4] (&&) Executing: command`)
- Implements skip logic with explanatory messages
- Arguments passed only to the final command in chains

**Parallel Execution**:
- Uses `std::thread` and `mpsc::channel` for coordination
- Reports completion status for each thread
- Aggregates results and provides summary feedback

## Key Features Implementation

**Advanced Command Chaining**:
- `--and` (&&): Execute if previous succeeded (exit code 0)
- `--or` (||): Execute if previous failed (exit code ≠ 0)
- `--always` (;): Always execute regardless of previous result
- `--if-code <N>`: Execute only if previous exit code equals N
- `--parallel`: Run all commands simultaneously

**Safety and UX**:
- Reserved namespace protection (no aliases starting with `--`, containing `mgr:`, or starting with `.`)
- Overwrite confirmation with current vs. new command comparison
- Colorized ANSI output throughout interface
- Interactive help system with optional detailed examples

## Testing Strategy

The codebase includes comprehensive unit tests covering:
- Configuration management (add/remove/list aliases)
- Serialization/deserialization of complex command chains
- Reserved name validation
- Legacy config file migration
- Manager state persistence

Tests use `tempfile` crate for isolated filesystem testing without affecting user configs.

## Error Handling Patterns

- **Result propagation**: Extensive use of `Result<T, String>` with proper error chaining
- **Graceful degradation**: Failed commands don't crash chains, they affect conditional flow
- **User feedback**: All errors include colorized output with helpful context
- **Exit code preservation**: Commands exit with same code as the underlying process for shell integration