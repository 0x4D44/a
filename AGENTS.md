# Repository Guidelines

## Project Structure & Module Organization
- Rust binary crate. Primary entrypoint: `src/main.rs`.
- Package manifest: `Cargo.toml`; docs: `README.md`.
- Build artifacts in `target/` (ignored by Git).
- Runtime configuration is a JSON alias store; a sample `config.json` exists for reference. The actual path is shown by `a --config`.

## Build, Test, and Development Commands
- Build (debug): `cargo build`
- Build (release): `cargo build --release`
- Run locally: `cargo run -- --help` or `cargo run -- <args>`
- Unit tests: `cargo test`
- Format: `cargo fmt --all`
- Lint (deny warnings): `cargo clippy -- -D warnings`

## Coding Style & Naming Conventions
- Use `rustfmt` defaults; run formatting before commits.
- Naming: modules/files `snake_case`; types/traits `CamelCase`; functions/vars `snake_case`; constants `SCREAMING_SNAKE_CASE`.
- Keep functions small and focused; prefer clear return types over implicit conversions.
- User-facing CLI output should use the existing color/style helpers in `src/main.rs` and remain concise.

## Testing Guidelines
- Prefer unit tests colocated under `#[cfg(test)]` in the same file or adjacent modules.
- Name tests `test_*` and keep them deterministic.
- Use `tempfile` for filesystem interactions; avoid touching real user config.
- If behavior changes are user-visible, add a minimal example to `README.md` and cover with a test.

## Commit & Pull Request Guidelines
- Commit messages: imperative mood and scoped, e.g. `feat(core): add chain operators` or `fix(windows): path handling`.
- For user-visible changes, bump version in `Cargo.toml` and update `const VERSION` in `src/main.rs`.
- PRs should include: what/why, brief implementation notes, manual test steps, and linked issues. For CLI changes, paste example commands and output.
- Ensure `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` pass locally before requesting review.

## Security & Configuration Tips
- Do not commit secrets. Treat alias commands as untrusted input; this tool executes commands without shell interpolation—preserve that property.
- Keep paths portable (Windows/Linux/macOS). Avoid hardcoded absolute paths in code and tests.
 - Enable local hooks to enforce formatting/lints: `git config core.hooksPath .githooks`
