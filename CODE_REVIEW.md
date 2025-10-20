# Code Review Report

**Project:** Alias Manager (`a`)  
**Date:** October 20, 2025  
**Reviewer:** ChatGPT (Codex)

## Summary
- Reviewed the primary Rust crate (`src/main.rs` ~2.3k LOC), top-level configuration, and companion Windows release script.
- Overall structure is clear and the feature set is well covered by unit tests, but several high-impact issues affect CLI usability and test hygiene.

## Findings

### High Severity
- **Quoted arguments break when executing aliases** — `src/main.rs:1015`, `src/main.rs:1044`, `src/main.rs:1087`  
  Aliases are split with `split_whitespace()`, so values containing spaces or quotes (e.g. `git commit -m "fix bug"`) are handed to child processes with stray quote characters or truncated arguments. This contradicts the documented goal of safe, shell-free execution and makes many real-world aliases unusable. Prefer a parser that understands shell-style quoting (e.g. `shell_words::split`) or store commands as structured `{ program, args[] }` tokens instead of a single string.
- **`a --help` blocks in non-interactive contexts** — `src/main.rs:1313-1338`  
  Displaying help text prompts the user (`Show detailed examples? (Y/n)`) and waits on `stdin`. When stdout is piped (e.g. `a --help | less` or scripting), this call blocks indefinitely. Help output must be non-interactive; detect TTY before prompting, add a `--no-examples` flag, or remove the prompt and show examples unconditionally.

### Medium Severity
- **Parameter substitution limited to single-digit indexes** — `src/main.rs:1147-1156`  
  `$10` becomes `$1` followed by literal `0`, so aliases cannot target arguments ≥ 10. Extend parsing to consume multi-digit numbers (e.g. read while `char::is_ascii_digit`) to support larger positional parameters.
- **Tests mutate global current directory without restoring** — `src/main.rs:2266`  
  `test_export_config_to_current_dir` switches the process CWD to a temp folder and never restores it. When the `TempDir` drops, the directory can disappear, leaving later tests in a removed location (especially flaky on Windows). Save the original directory, wrap the change in a guard, or use `assert_fs` utilities that auto-clean.
- **`test_config_path_creation` writes into the real home directory** — `src/main.rs:2016-2024`  
  `AliasManager::get_config_path()` creates `~/.alias-mgr` if it is absent, so this test pollutes developer machines/CI environments despite the comment claiming otherwise. For tests, override `HOME`/`USERPROFILE` to a temp dir or expose an injectable base path.

### Low Severity
- **Unused dependency** — `Cargo.toml:10`  
  The `colored` crate is declared but unused; removing it will shrink compile times and dependency surface area unless future work relies on it.

## Positive Notes
- Comprehensive unit tests cover configuration persistence, parameter substitution, and export flows.
- GitHub sync features include defensive checks (backups before overwriting, SHA reuse).
- CLI output consistently uses centralized color constants, keeping UX polished.

## Recommendations
1. Replace whitespace splitting with robust argument tokenization for all execution paths.
2. Make the help command non-interactive by default; surface examples via a dedicated flag.
3. Harden parameter substitution and test utilities to avoid global side effects.
4. Trim unused dependencies and revisit tests that touch user environments.

