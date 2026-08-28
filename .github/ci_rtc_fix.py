#!/usr/bin/env python3
"""Replace the provisional TOD clock with an ICM7170 periodic model."""

from pathlib import Path

RTC_SOURCE = r'''//! Deterministic Intersil ICM7170 time-of-day clock.

/// Nominal Sun 3/60 processor frequency used by the virtual clock.
const CPU_HZ: u64 = 20_000_000;
/// The ICM7170 advances its hundredths counter at 100 Hz.
const CLOCK_HZ: u64 = 100;
/// Guest CPU cycles per one-hundredth-second clock edge.
const CYCLES_PER_TICK: u64 = CPU_HZ / CLOCK_HZ;

const REG_HUNDREDTH: usize = 0x00;
const REG_HOURS: usize = 0x01;
const REG_MINUTES: usize = 0x02;
const REG_SECONDS: usize = 0x03;
const REG_MONTH: usize = 0x04;
const REG_DAY: usize = 0x05;
const REG_YEAR: usize = 0x06;
const REG_DAY_OF_WEEK: usize = 0x07;
const REG_ALARM_HUNDREDTH: usize = 0x08;
const REG_ALARM_HOURS: usize = 0x09;
const REG_ALARM_MINUTES: usize = 0x0a;
const REG_ALARM_SECONDS: usize = 0x0b;
const REG_ALARM_MONTH: usize = 0x0c;
const REG_ALARM_DAY: usize = 0x0d;
const REG_ALARM_YEAR: usize = 0x0e;
const REG_STATUS_MASK: usize = 0x10;
const REG_COMMAND: usize = 0x11;

const COMMAND_TEST_MODE: u8 = 0x20;
const COMMAND_IRQ_ENABLE: u8 = 0x10;
const COMMAND_RUN: u8 = 0x08;
const COMMAND_24_HOUR: u8 = 0x04;

const IRQ_GLOBAL: u8 = 0x80;
const IRQ_DAY: u8 = 0x40;
const IRQ_HOUR: u8 = 0x20;
const IRQ_MINUTE: u8 = 0x10;
const IRQ_SECOND: u8 = 0x08;
const IRQ_TENTH: u8 = 0x04;
const IRQ_HUNDREDTH: u8 = 0x02;
const IRQ_ALARM: u8 = 0x01;

/// Stateful ICM7170 model driven entirely by guest cycles.
#[derive(Debug, Clone)]
pub struct Intersil7170 {
    registers: [u8; 0x20],
    cycle_accumulator: u64,
    irq_mask: u8,
    irq_status: u8,
}

impl Default for Intersil7170 {
    fn default() -> Self {
        let mut registers = [0_u8; 0x20];
        // Deterministic Friday, 1 January 1988, 12:00:00.00.
        registers[REG_HOURS] = 12;
        registers[REG_MONTH] = 1;
        registers[REG_DAY] = 1;
        registers[REG_YEAR] = 88;
        registers[REG_DAY_OF_WEEK] = 5;
        // Wild-card alarm fields until firmware programs them.
        registers[REG_ALARM_HUNDREDTH..=REG_ALARM_YEAR].fill(0x80);
        Self {
            registers,
            cycle_accumulator: 0,
            irq_mask: 0,
            irq_status: 0,
        }
    }
}

impl Intersil7170 {
    /// Reset run/interrupt state while retaining battery-backed counters/RAM.
    pub fn reset(&mut self) {
        self.registers[REG_COMMAND] &= !COMMAND_RUN;
        self.cycle_accumulator = 0;
        self.irq_status = 0;
    }

    /// Advance the device by deterministic guest CPU cycles.
    pub fn advance(&mut self, cycles: u64) {
        if self.registers[REG_COMMAND] & COMMAND_RUN == 0 {
            return;
        }

        self.cycle_accumulator = self.cycle_accumulator.saturating_add(cycles);
        while self.cycle_accumulator >= CYCLES_PER_TICK {
            self.cycle_accumulator -= CYCLES_PER_TICK;
            self.clock_tick();
        }
    }

    /// Read one byte-wide register.
    pub fn read(&mut self, offset: u32) -> u8 {
        let index = offset as usize & 0x1f;
        if index == REG_STATUS_MASK {
            let status = self.status_with_global_bit();
            self.irq_status = 0;
            return status;
        }
        self.registers[index]
    }

    /// Write one byte-wide register.
    pub fn write(&mut self, offset: u32, value: u8) {
        let index = offset as usize & 0x1f;
        match index {
            REG_STATUS_MASK => {
                self.irq_mask = value & !IRQ_GLOBAL;
            }
            REG_COMMAND => {
                let was_running = self.registers[REG_COMMAND] & COMMAND_RUN != 0;
                self.registers[REG_COMMAND] = value;
                let running = value & COMMAND_RUN != 0;
                if running && !was_running {
                    // A newly started clock begins a complete hundredth period.
                    self.cycle_accumulator = 0;
                } else if !running {
                    self.cycle_accumulator = 0;
                }
            }
            _ => self.registers[index] = value,
        }
    }

    /// Current active-low interrupt output represented as an asserted request.
    #[must_use]
    pub fn interrupt_pending(&self) -> bool {
        self.registers[REG_COMMAND] & COMMAND_IRQ_ENABLE != 0
            && self.irq_status & self.irq_mask & !IRQ_GLOBAL != 0
    }

    fn status_with_global_bit(&self) -> u8 {
        let mut status = self.irq_status & !IRQ_GLOBAL;
        if status & self.irq_mask != 0 {
            status |= IRQ_GLOBAL;
        }
        status
    }

    fn clock_tick(&mut self) {
        self.irq_status |= IRQ_HUNDREDTH;
        self.registers[REG_HUNDREDTH] += 1;

        if self.registers[REG_HUNDREDTH].is_multiple_of(10) {
            self.irq_status |= IRQ_TENTH;
        }

        if self.registers[REG_HUNDREDTH] >= 100 {
            self.registers[REG_HUNDREDTH] = 0;
            self.irq_status |= IRQ_SECOND;
            self.increment_seconds();
        }

        if self.alarm_matches() {
            self.irq_status |= IRQ_ALARM;
        }
    }

    fn increment_seconds(&mut self) {
        self.registers[REG_SECONDS] += 1;
        if self.registers[REG_SECONDS] < 60 {
            return;
        }
        self.registers[REG_SECONDS] = 0;
        self.irq_status |= IRQ_MINUTE;

        self.registers[REG_MINUTES] += 1;
        if self.registers[REG_MINUTES] < 60 {
            return;
        }
        self.registers[REG_MINUTES] = 0;
        self.irq_status |= IRQ_HOUR;
        self.increment_hours();
    }

    fn increment_hours(&mut self) {
        if self.registers[REG_COMMAND] & COMMAND_24_HOUR != 0 {
            self.registers[REG_HOURS] += 1;
            if self.registers[REG_HOURS] < 24 {
                return;
            }
            self.registers[REG_HOURS] = 0;
        } else {
            let pm = self.registers[REG_HOURS] & 0x80;
            let mut hour = self.registers[REG_HOURS] & 0x0f;
            if hour == 11 {
                self.registers[REG_HOURS] = (pm ^ 0x80) | 12;
                return;
            }
            hour = if hour >= 12 { 1 } else { hour + 1 };
            self.registers[REG_HOURS] = pm | hour;
            return;
        }

        self.irq_status |= IRQ_DAY;
        self.increment_day();
    }

    fn increment_day(&mut self) {
        let year = 1900_u16 + u16::from(self.registers[REG_YEAR]);
        let month = self.registers[REG_MONTH].clamp(1, 12);
        let last_day = days_in_month(year, month);

        self.registers[REG_DAY] += 1;
        self.registers[REG_DAY_OF_WEEK] = (self.registers[REG_DAY_OF_WEEK] + 1) % 7;
        if self.registers[REG_DAY] <= last_day {
            return;
        }

        self.registers[REG_DAY] = 1;
        self.registers[REG_MONTH] += 1;
        if self.registers[REG_MONTH] <= 12 {
            return;
        }
        self.registers[REG_MONTH] = 1;
        self.registers[REG_YEAR] = (self.registers[REG_YEAR] + 1) % 100;
    }

    fn alarm_matches(&self) -> bool {
        // ICM7170 alarm bytes use the high bit as a don't-care selector.
        const PAIRS: [(usize, usize, u8); 7] = [
            (REG_ALARM_HUNDREDTH, REG_HUNDREDTH, 0x7f),
            (REG_ALARM_HOURS, REG_HOURS, 0x8f),
            (REG_ALARM_MINUTES, REG_MINUTES, 0x3f),
            (REG_ALARM_SECONDS, REG_SECONDS, 0x3f),
            (REG_ALARM_MONTH, REG_MONTH, 0x0f),
            (REG_ALARM_DAY, REG_DAY, 0x1f),
            (REG_ALARM_YEAR, REG_YEAR, 0x7f),
        ];
        PAIRS.iter().all(|&(alarm, counter, mask)| {
            self.registers[alarm] & 0x80 != 0
                || (self.registers[alarm] ^ self.registers[counter]) & mask == 0
        })
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_1c_and_hundredth_mask_generate_periodic_irqs() {
        let mut rtc = Intersil7170::default();
        rtc.write(REG_STATUS_MASK as u32, IRQ_HUNDREDTH);
        rtc.write(REG_COMMAND as u32, 0x1c);

        rtc.advance(CYCLES_PER_TICK - 1);
        assert!(!rtc.interrupt_pending());
        rtc.advance(1);
        assert!(rtc.interrupt_pending());
        assert_eq!(rtc.read(REG_STATUS_MASK as u32), IRQ_GLOBAL | IRQ_HUNDREDTH);
        assert!(!rtc.interrupt_pending());

        rtc.advance(CYCLES_PER_TICK);
        assert!(rtc.interrupt_pending());
    }

    #[test]
    fn interrupt_acknowledge_does_not_replace_status_register_clear() {
        let mut rtc = Intersil7170::default();
        rtc.write(REG_STATUS_MASK as u32, IRQ_TENTH);
        rtc.write(REG_COMMAND as u32, COMMAND_RUN | COMMAND_IRQ_ENABLE | COMMAND_24_HOUR);
        rtc.advance(CYCLES_PER_TICK * 10);
        assert!(rtc.interrupt_pending());
        assert_eq!(rtc.read(REG_STATUS_MASK as u32), IRQ_GLOBAL | IRQ_TENTH | IRQ_HUNDREDTH);
        assert!(!rtc.interrupt_pending());
    }

    #[test]
    fn one_hundred_ticks_advance_one_second() {
        let mut rtc = Intersil7170::default();
        rtc.write(REG_COMMAND as u32, COMMAND_RUN | COMMAND_24_HOUR);
        rtc.advance(CYCLES_PER_TICK * 100);
        assert_eq!(rtc.read(REG_HUNDREDTH as u32), 0);
        assert_eq!(rtc.read(REG_SECONDS as u32), 1);
    }

    #[test]
    fn masked_sources_do_not_assert_the_pin() {
        let mut rtc = Intersil7170::default();
        rtc.write(REG_COMMAND as u32, COMMAND_RUN | COMMAND_IRQ_ENABLE);
        rtc.advance(CYCLES_PER_TICK);
        assert!(!rtc.interrupt_pending());
    }

    #[test]
    fn test_mode_bit_does_not_disable_normal_clocking() {
        let mut rtc = Intersil7170::default();
        rtc.write(REG_COMMAND as u32, COMMAND_TEST_MODE | COMMAND_RUN);
        rtc.advance(CYCLES_PER_TICK);
        assert_eq!(rtc.read(REG_HUNDREDTH as u32), 1);
    }
}
'''


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old not in text:
        raise SystemExit(f"expected RTC edit not found in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1))


def main() -> None:
    Path("crates/sun3-machine/src/devices/rtc.rs").write_text(RTC_SOURCE)
    replace_once(
        Path("crates/sun3-machine/src/bus.rs"),
        """        if matches!(level, 5 | 7) && self.rtc.interrupt_pending() {
            self.rtc.acknowledge_interrupt();
        }
        u32::MAX
""",
        """        // Device state is cleared by the guest reading the ICM7170
        // status register, not by the 68020 interrupt-acknowledge cycle.
        u32::MAX
""",
    )


if __name__ == "__main__":
    main()
