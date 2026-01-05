# Coverage Improvement Plan (98% Target) - Status Update

## Completed Stages
- [x] **Stage 1: Legacy Support:** Implemented tests for legacy chains and migration.
- [x] **Stage 2: GitHub Token & CLI Parsing:** Refactored `github_token` logic into `TokenProvider` and added tests using `MockOutputCommandRunner`. Added CLI smoke tests for error cases.
- [x] **Stage 3: Windows Path Resolution:** Added unit tests for `SystemCommandRunner` edge cases.
- [x] **Stage 4: Parallel Execution:** Covered thread panic handling.

## Current Status
Coverage stands at **91.1%**. Reaching 98% would require:
1.  Mocking `std::env` and `std::fs` entirely for `SystemCommandRunner` (currently uses real environment with guards).
2.  Mocking `std::io::stdin/stdout` for all CLI interactions (currently `println!` macros are used directly).
3.  Extracting `main` logic into a fully testable `run_app` function that takes injected dependencies (FileSystem, Console, Env).

## Recommendation
The current level of coverage is excellent for a system tool. Further refactoring to reach 98% yields diminishing returns versus code complexity.