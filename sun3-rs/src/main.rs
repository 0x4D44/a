use m68k::{CpuCore, CpuType, LinearMemoryBus, StepResult};

fn main() {
    let mut memory = LinearMemoryBus::new(1024 * 1024);
    memory.write_long_at(0, 0x0008_0000);
    memory.write_long_at(4, 0x0000_1000);
    memory.write_word_at(0x1000, 0x4e71);

    let mut cpu = CpuCore::new();
    cpu.set_cpu_type(CpuType::M68020);
    cpu.fpu_present = true;
    cpu.reset(&mut memory);

    match cpu.step(&mut memory) {
        StepResult::Ok { cycles } => {
            println!("m68k foundation ok: pc={:#010x}, cycles={cycles}", cpu.pc);
        }
        other => panic!("unexpected CPU result: {other:?}"),
    }
}
