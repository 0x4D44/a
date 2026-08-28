#!/usr/bin/env python3
"""Apply compiler-guided cleanup to the staged source archive.

This file is temporary scaffolding while the canonical source archive is being
iterated through GitHub Actions. The final packaged source contains these edits
in-place and does not depend on this script at runtime.
"""

from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old not in text:
        raise SystemExit(f"expected edit not found in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1))


def main() -> None:
    replace_once(
        Path("crates/sun3-machine/src/machine.rs"),
        "self.ram_bytes % RAM_STEP_BYTES != 0",
        "!self.ram_bytes.is_multiple_of(RAM_STEP_BYTES)",
    )
    replace_once(
        Path("crates/sun3-machine/src/mmu.rs"),
        "    #[must_use]\n    pub fn translate_peek(",
        "    pub fn translate_peek(",
    )
    replace_once(
        Path("crates/sun3-machine/src/trace.rs"),
        "capacity.min(4096).max(1)",
        "capacity.clamp(1, 4096)",
    )
    replace_once(
        Path("crates/sun3-cli/src/main.rs"),
        """    if report.reached_prompt() {
        if let Some(command) = &cli.command {
            let mut bytes = command.clone();
            if !bytes.ends_with(b"\\r") && !bytes.ends_with(b"\\n") {
                bytes.push(b'\\r');
            }
            machine.push_console_bytes(&bytes);
            let command_report = machine.run(&RunOptions {
                max_instructions: cli.max_instructions,
                prompt: cli.prompt.clone(),
                stop_on_prompt: true,
            });
            command_ok = command_report.reached_prompt();
        }
    }
""",
        """    if report.reached_prompt()
        && let Some(command) = &cli.command
    {
        let mut bytes = command.clone();
        if !bytes.ends_with(b"\\r") && !bytes.ends_with(b"\\n") {
            bytes.push(b'\\r');
        }
        machine.push_console_bytes(&bytes);
        let command_report = machine.run(&RunOptions {
            max_instructions: cli.max_instructions,
            prompt: cli.prompt.clone(),
            stop_on_prompt: true,
        });
        command_ok = command_report.reached_prompt();
    }
""",
    )
    replace_once(
        Path("crates/sun3-cli/src/main.rs"),
        """        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
""",
        """        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
""",
    )


if __name__ == "__main__":
    main()
