# Coverage Audit and Improvement Plan (October 20, 2025)

## Coverage Snapshot
- Tooling: `cargo llvm-cov 0.6.20` (invoked via `cargo llvm-cov --json --summary-only` on October 20, 2025)
- Test suite: 25 unit tests (all green)
- Metrics:
  - Line coverage: **41.94%** (640 / 1,526 lines)
  - Function coverage: **45.19%** (47 / 104 functions)
  - Region coverage: **44.99%** (1,037 / 2,305 regions)
- Generated artifacts: `coverage-summary.json` and `coverage-report.txt`

## High-Risk Gaps (Zero-Coverage Functions)
All located in `src/main.rs`:

| Function | Line | Risk Summary |
| --- | --- | --- |
| `AliasManager::new` | 159 | Config bootstrap path never exercised; HOME/USERPROFILE error handling unverified. |
| `AliasManager::list_aliases` | 512 | CLI-facing formatting/empty states not validated. |
| `AliasManager::migrate_legacy_config` | 206 | Legacy config compatibility untested; regression risk for existing users. |
| `AliasManager::github_token` | 253 | Token precedence logic unchecked. |
| `AliasManager::push_config_to_github` | 260 | Full GitHub push workflow untested (encoding, auth, error paths). |
| `AliasManager::pull_config_from_github` | 343 | Download/backups/unicode handling untested. |
| `AliasManager::confirm_overwrite` | 492 | Interactive overwrite path unverified; stdin/out handling fragile. |
| `AliasManager::which_alias` | 563 | Alias inspection output formatting uncovered. |
| `AliasManager::show_config_location` | 659 | Config path messaging untested. |
| `AliasManager::execute_alias` | 724 | Entry to execution pipeline uncovered (simple/chain/parallel branches). |
| `AliasManager::execute_legacy_command_chain` | 749 | Legacy `&&` chain support unverified. |
| `AliasManager::execute_sequential_chain` | 803 | Sequential chain control flow and exit-code logic untested. |
| `AliasManager::execute_parallel_chain` | 893 | Parallel execution orchestration untested; potential race/cleanup bugs. |
| `AliasManager::execute_single_command_with_exit_code` | 1003 | Return-code propagation never asserted. |
| `AliasManager::execute_command_static` | 1025 | Static helper for non-interactive trim unset; possible panic on empty command. |
| `AliasManager::execute_single_command` | 1043 | Primary execution path (including exit escalation) unused in tests. |
| `print_help` | 1178 | Primary UX text for CLI untested (risk of regressions when editing copy). |
| `print_examples` | 1304 | Extended help content untested. |
| `print_version` | 1440 | Version banner unchecked; tied to release workflow. |
| `main` | 1452 | CLI argument routing entirely uncovered; high regression surface. |

## Multi-Stage Coverage Improvement Plan
The goal is to raise line coverage above 80% and function coverage above 75% while exercising every user-facing CLI branch. Each stage is incremental and can land via separate PRs.

### Stage 1 – CLI Smoke & Config Bootstrap (Target +15% lines / +12% functions)
- Add binary integration tests using `assert_cmd` to spawn the CLI via `cargo_bin!("a")`:
  - `a --help`, `a --help --examples`, `a --version`, `a --config` paths.
  - No-argument invocation (should print help and exit 0).
  - Unknown flag errors to cover stderr pathways.
- Use `assert_fs`/`tempfile` to isolate config directories, setting `HOME`/`USERPROFILE` via guard helpers so that `AliasManager::new` and `show_config_location` execute without touching real user state.
- Seed a minimal JSON config to drive `which_alias` output assertions.
- Create regression tests for missing HOME/USERPROFILE to validate error messaging.

### Stage 2 – Legacy & Listing Behavior (Target +8% lines / +6% functions)
- Introduce fixture legacy config JSON (with `command`) and call `AliasManager::load_config` to force `migrate_legacy_config`.
- Test `list_aliases` filtering/formatting by capturing stdout (use `assert_cmd` + `Command::cargo_bin` or `duct` to redirect) ensuring alignment logic and empty states hit.
- Refactor `confirm_overwrite` to accept a `Read`/`Write` pair behind a small injected trait (defaulting to stdin/stdout) so tests can simulate `y/n` responses without blocking. Add unit tests covering `yes`, `no`, and read errors.
- Cover `which_alias` output (with description, chaining, and parameter hints) using captured stdout assertions.

### Stage 3 – Execution Pipeline (Target +12% lines / +10% functions)
- Extract a lightweight `CommandRunner` trait with real and test doubles to allow deterministic command execution without spawning external processes.
- Write tests for:
  - `execute_alias` routing for simple, legacy `&&`, sequential chain, and parallel chain.
  - Sequential chain stop-on-failure / conditional operators (AND/OR/Always/IfCode).
  - Parallel chain success/failure aggregation and join semantics (use fake runner to assert concurrency orchestration without real threads).
  - `execute_single_command_with_exit_code` and `execute_command_static` return values, including empty command errors.
- Add guard tests ensuring $-parameter expansion propagates into execution (bridging to existing substitution coverage).

### Stage 4 – GitHub Sync Workflows (Target +6% lines / +5% functions)
- Enable `ureq = { version = "*, features = ["json", "mock"] }` in dev/test profile to leverage `ureq::Agent::mock` for deterministic HTTP interactions.
- Add tests for `github_token` precedence order (A_GITHUB_TOKEN → GITHUB_TOKEN → GH_TOKEN).
- Simulate push scenarios:
  - Existing file (GET 200) with SHA and successful PUT (201).
  - Missing file (GET 404) leading to creation.
  - Error propagation (e.g., PUT 500) to assert messaging.
- Simulate pull scenarios:
  - Valid base64 config, backup creation, and stdout messaging.
  - Non-base64 encoding response triggering error.
  - Missing content / network errors.
- Use `assert_fs::TempDir` to confirm config/backup writes occur in fixture directories.

### Stage 5 – Automation & Regression Guardrails (Target +3% lines / +2% functions)
- Add a README section documenting how to run `cargo llvm-cov` locally and set coverage expectations.
- Wire `cargo llvm-cov --report html` into CI optional job (or document manual step) to prevent regressions.
- Track coverage trend by storing baseline `coverage-summary.json` diff in CI artifacts (optional but recommended).

## Verification Checklist per Stage
1. `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` must remain green.
2. `cargo llvm-cov --json --summary-only --output-path coverage-summary.json` to record improvements.
3. Update `README.md` when user-visible behavior (help/version output) is exercised or documented.
4. Ensure new tests are hermetic: rely on `TempDir`, env guards, and runners that do not mutate global state beyond their scope.

With these stages completed, we project overall coverage to exceed the 80/75 (line/function) thresholds while putting every critical CLI and synchronization path under test.

## Progress Log
- **October 20, 2025 – Stage 1 complete:** Added CLI smoke tests (`tests/cli_smoke.rs`) covering `--help`, `--help --examples`, `--version`, `--config`, `--which`, and error/missing HOME scenarios. Line coverage rose to 55.05%; function coverage to 51.92%.
- **October 20, 2025 – Stage 2 complete:** Refactored `confirm_overwrite` for injectable IO (`src/main.rs:492`) and added legacy migration plus list-formatting tests. Line coverage now **60.58%**; function coverage **57.52%** (`coverage-summary.json`).
- **October 20, 2025 – Stage 3 complete:** Introduced injectable `CommandRunner` with mockable harness, covered `execute_alias`, sequential and parallel chains (`src/main.rs:527`–`1015`), and added focused tests for command routing (`src/main.rs:2554`). Coverage up to **72.19%** lines and **69.67%** functions (`coverage-summary.json`).
- **October 20, 2025 – Stage 4 complete:** Added injectable GitHub client, refactoring push/pull sync logic for deterministic testing (`src/main.rs:78`–`455`), and built mocks covering token precedence, push success/failure paths, and pull backups (`src/main.rs:2707`). Coverage now **79.24%** lines and **76.62%** functions (`coverage-summary.json`).
- **October 20, 2025 – Stage 5 complete:** CLI coverage now spans `--push/--pull/--export`, chained `--which` output, and the real `SystemCommandRunner`; coverage workflow documented in `README.md`; GitHub sync exercised with success/failure + transport error cases; help/version printers invoked directly; CI now runs `cargo llvm-cov --summary-only` with ≥80 %/≥75 % gates and stores the JSON summary. Current metrics **86.21%** lines / **83.63%** functions (`coverage-summary.json`).
