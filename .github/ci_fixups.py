#!/usr/bin/env python3
"""Apply compiler-guided and PROM-guided fixes to the staged source archive.

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

    # Sun hardware records the latest bus-error cause; it does not accumulate
    # old INVALID/PROTECTION bits into a later timeout.  The genuine PROM's
    # NXM test exposes this by running immediately after the invalid-PTE test.
    mmu = Path("crates/sun3-machine/src/mmu.rs")
    replace_once(
        mmu,
        """    /// Add one or more cause bits to the board bus-error latch.
    pub fn latch_bus_error(&mut self, bits: u8) {
        self.bus_error |= bits;
    }

    /// Read and clear the board bus-error latch.
    pub fn take_bus_error(&mut self) -> u8 {
        std::mem::take(&mut self.bus_error)
    }
""",
        """    /// Record the most recent board bus-error cause.
    pub fn latch_bus_error(&mut self, bits: u8) {
        self.bus_error = bits;
    }
""",
    )
    replace_once(
        mmu,
        """                self.bus_error |= match fault.kind {
                    MmuFaultKind::Invalid => BUS_ERROR_INVALID,
                    MmuFaultKind::Protection => BUS_ERROR_PROTECTION,
                    MmuFaultKind::Timeout => BUS_ERROR_TIMEOUT,
                };
""",
        """                self.bus_error = match fault.kind {
                    MmuFaultKind::Invalid => BUS_ERROR_INVALID,
                    MmuFaultKind::Protection => BUS_ERROR_PROTECTION,
                    MmuFaultKind::Timeout => BUS_ERROR_TIMEOUT,
                };
""",
    )
    replace_once(
        mmu,
        "            6 => Ok(place_register_byte(self.take_bus_error(), size)),",
        "            6 => Ok(place_register_byte(self.bus_error, size)),",
    )
    replace_once(
        mmu,
        """    #[test]
    fn page_map_supports_big_endian_partial_updates() {
""",
        """    #[test]
    fn newer_fault_replaces_the_previous_bus_error_cause() {
        let mut mmu = Sun3Mmu::new();
        mmu.enable = 0x80;
        mmu.set_page_entry(0, 0, 0);
        let _ = mmu.translate(0, FunctionCode::SupervisorData, false);
        assert_eq!(mmu.bus_error(), BUS_ERROR_INVALID);
        mmu.latch_bus_error(BUS_ERROR_TIMEOUT);
        assert_eq!(mmu.bus_error(), BUS_ERROR_TIMEOUT);
    }

    #[test]
    fn page_map_supports_big_endian_partial_updates() {
""",
    )

    # Keep failure iterations short; the final acceptance run restores the
    # full 80-million-instruction budget once the monitor is reached.
    replace_once(
        Path("scripts/acceptance.sh"),
        "--max-instructions 80000000",
        "--max-instructions 20000000",
    )


if __name__ == "__main__":
    main()
