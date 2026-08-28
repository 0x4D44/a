#!/usr/bin/env python3
"""Tighten details of the generated ICM7170 model before compilation."""

from pathlib import Path


def replace_once(text: str, old: str, new: str) -> str:
    if old not in text:
        raise SystemExit(f"expected RTC polish edit not found: {old!r}")
    return text.replace(old, new, 1)


def main() -> None:
    path = Path("crates/sun3-machine/src/devices/rtc.rs")
    text = path.read_text()
    text = replace_once(
        text,
        "const COMMAND_TEST_MODE: u8 = 0x20;",
        "#[cfg(test)]\nconst COMMAND_TEST_MODE: u8 = 0x20;",
    )
    text = replace_once(
        text,
        "        if self.alarm_matches() {",
        "        if self.irq_mask & IRQ_ALARM != 0 && self.alarm_matches() {",
    )
    text = replace_once(
        text,
        """            if hour == 11 {
                self.registers[REG_HOURS] = (pm ^ 0x80) | 12;
                return;
            }
""",
        """            if hour == 11 {
                self.registers[REG_HOURS] = (pm ^ 0x80) | 12;
                if pm != 0 {
                    self.irq_status |= IRQ_DAY;
                    self.increment_day();
                }
                return;
            }
""",
    )
    path.write_text(text)


if __name__ == "__main__":
    main()
