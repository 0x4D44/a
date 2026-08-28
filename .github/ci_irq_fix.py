#!/usr/bin/env python3
"""Add MC68020 level-seven edge sampling to the Sun board adapter."""

from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old not in text:
        raise SystemExit(f"expected IRQ edit not found in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1))


def main() -> None:
    machine = Path("crates/sun3-machine/src/machine.rs")
    replace_once(
        machine,
        """pub struct Sun3Machine {
    cpu: CpuCore,
    bus: Sun3Bus,
    instructions: u64,
}
""",
        """pub struct Sun3Machine {
    cpu: CpuCore,
    bus: Sun3Bus,
    instructions: u64,
    last_board_irq: u8,
}
""",
    )
    replace_once(
        machine,
        """        Ok(Self {
            cpu,
            bus,
            instructions: 0,
        })
""",
        """        Ok(Self {
            cpu,
            bus,
            instructions: 0,
            last_board_irq: 0,
        })
""",
    )
    replace_once(
        machine,
        """        self.bus.set_reset_vector_mode(false);
        self.instructions = 0;
    }
""",
        """        self.bus.set_reset_vector_mode(false);
        self.instructions = 0;
        self.last_board_irq = 0;
    }
""",
    )
    replace_once(
        machine,
        """        let irq_level = self.bus.irq_level();
        self.cpu.set_irq(irq_level);
""",
        """        let board_irq = self.bus.irq_level();
        let irq_level = present_irq_level(board_irq, self.last_board_irq);
        self.last_board_irq = board_irq;
        self.cpu.set_irq(irq_level);
""",
    )
    replace_once(
        machine,
        """fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
""",
        """fn present_irq_level(board_level: u8, previous_board_level: u8) -> u8 {
    // IPL7 is the 68020's non-maskable interrupt input. It is recognized on
    // the transition into level seven, not repeatedly while a device holds
    // the line high. Lower levels remain level-sensitive.
    if board_level == 7 && previous_board_level == 7 {
        0
    } else {
        board_level
    }
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
""",
    )
    replace_once(
        machine,
        """    #[test]
    fn genuine_cpu_executes_prom_words() {
""",
        """    #[test]
    fn level_seven_is_presented_once_per_rising_edge() {
        assert_eq!(present_irq_level(7, 0), 7);
        assert_eq!(present_irq_level(7, 7), 0);
        assert_eq!(present_irq_level(0, 7), 0);
        assert_eq!(present_irq_level(7, 0), 7);
        assert_eq!(present_irq_level(5, 5), 5);
    }

    #[test]
    fn genuine_cpu_executes_prom_words() {
""",
    )

    # Leave enough execution budget for POST, the full 4 MiB memory walk, the
    # PROM's ten-second menu window, and its no-device autoboot fallback.
    replace_once(
        Path("scripts/acceptance.sh"),
        "--max-instructions 20000000",
        "--max-instructions 160000000",
    )


if __name__ == "__main__":
    main()
