# Remediation Plan for Alias Manager Review Findings

## Stage 1 – CLI Runtime Hardening
- Replace all `split_whitespace` usage in command execution helpers with a robust tokenizer (e.g. integrate `shell_words::split` or store commands as `{ program, args[] }`) so quoted arguments and embedded spaces round-trip correctly.
- Refactor `a --help` to avoid interactive prompts unless explicitly requested (for example, detect TTY before asking or add a `--examples` flag) and make sure help output never blocks in pipelines.
- Extend parameter substitution to support multi-digit positional arguments by consuming consecutive digits when parsing `$N`.

## Stage 2 – Test & Environment Hygiene
- Update tests that mutate global process state (`env::set_current_dir`) to restore the original working directory automatically (use RAII guard or scoped helper).
- Adjust `test_config_path_creation` (and related code if needed) to run against a temporary home directory by overriding `HOME`/`USERPROFILE`, preventing writes to real user locations.

## Stage 3 – Dependency & Follow-up Cleanup
- Remove the unused `colored` dependency from `Cargo.toml` (and `Cargo.lock`) after confirming no code paths rely on it.
- Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` to validate the remediation, then update docs/examples if CLI behavior changes (e.g. documenting new help flags).
