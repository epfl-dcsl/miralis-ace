//! Tracing firmware payload
//!
//! This payload measure the cost of a context switch in two situations
//! Situation 1: VM-mode firmware <--> Miralis
//! Situation 2: S-mode payload <--> VM-mode firmware

#![no_std]
#![no_main]

use core::arch::{asm, global_asm};

use miralis_abi::{failure, log, setup_binary, success};

setup_binary!(main);

fn enable_mcycle_in_smode() {
    unsafe {
        // This allows to read cycle in S-mode - for the payload
        let mcounteren: u32;
        asm!("csrr {}, mcounteren", out(reg) mcounteren);
        asm!("csrw mcounteren, {}", in(reg) mcounteren | 1);
    }
}

fn main() -> ! {
    let trap: usize = _empty_handler as usize;

    enable_mcycle_in_smode();

    unsafe {
        asm!(
        "csrw mtvec, {mtvec}", // Write mtvec with trap handler
        mtvec = in(reg) trap,
        );
    }

    log::info!("Start benchmarking from Firmware");

    measure();

    log::info!("Start benchmarking from Payload");

    let os: usize = operating_system as usize;
    let mpp = 0b1 << 11; // MPP = S-mode

    unsafe {
        asm!(
        "li t4, 0xfffffffff",
        "csrw pmpcfg0, 0xf",   // XRW TOR
        "csrw pmpaddr0, t4",   // All memory
        "auipc t4, 0",
        "addi t4, t4, 24",
        "csrw mstatus, {mpp}", // Write MPP of mstatus to S-mode
        "csrw mepc, {os}",     // Write MEPC
        "mret",                // Jump to OS


        os = in(reg) os,
        mpp = in(reg) mpp,
        out("t4") _,
        );
    }

    failure();
}

// —————————————————————————————— Trap Handler —————————————————————————————— //

global_asm!(
    r#"
.text
.align 4
.global _empty_handler
_empty_handler:
    // Skip illegal instruction (pc += 4)
    csrrw x5, mepc, x5
    addi x5, x5, 4
    csrrw x5, mepc, x5
    // Return back to OS
    mret
"#,
);

extern "C" {
    fn _empty_handler();
}

// —————————————————————————————— Benchmark operating system —————————————————————————————— //

const NB_REPEATS: usize = 1000;

pub fn bubble_sort(arr: &mut [usize; NB_REPEATS]) {
    let len = arr.len();
    let mut swapped;

    for i in 0..len {
        swapped = false;

        for j in 0..len - 1 - i {
            if arr[j] > arr[j + 1] {
                arr.swap(j, j + 1);
                swapped = true;
            }
        }

        if !swapped {
            break;
        }
    }

    for i in 1..len {
        if arr[i - 1] > arr[i] {
            log::error!("Error in sorting, results aren't reliable");
            failure();
        }
    }
}

fn operating_system() {
    unsafe {
        asm!("la sp, 0x80700000");
    }

    measure();
    success();
}

fn measure() {
    let mut values: [usize; NB_REPEATS] = [0; NB_REPEATS];

    for i in 0..NB_REPEATS {
        values[i] = trigger_ctx_switch_to_firmware()
    }

    let stats = get_statistics(values);
    let average_measure = trigger_ctx_switch_to_firmware_batched();

    log::info!("Average measure : {}", average_measure);
    print_statistics(stats);
}

fn trigger_ctx_switch_to_firmware() -> usize {
    let begin: u64;
    let end: u64;

    unsafe {
        // Read the `mcycle` register (assuming 64-bit RISC-V)
        asm!("csrr {}, cycle", out(reg) begin);
        // We trigger an illegal instruction
        asm!("csrw mscratch, zero");
        // Read the `mcycle` register (assuming 64-bit RISC-V)
        asm!("csrr {}, cycle", out(reg) end);
    }

    (end - begin) as usize
}

fn trigger_ctx_switch_to_firmware_batched() -> usize {
    let begin: u64;
    let end: u64;

    unsafe {
        // Read the `mcycle` register (assuming 64-bit RISC-V)
        asm!("csrr {}, cycle", out(reg) begin);
        for _ in 0..NB_REPEATS {
            // We can trigger an illegal instruction
            asm!("csrw mscratch, zero");
        }

        // Read the `mcycle` register (assuming 64-bit RISC-V)
        asm!("csrr {}, cycle", out(reg) end);
    }

    (end - begin) as usize / NB_REPEATS
}

#[derive(Debug)]
pub struct Statistics {
    mean: usize,
    min: usize,
    max: usize,

    p25: usize,
    p50: usize,
    p75: usize,
    p95: usize,
    p99: usize,
}

fn get_statistics(mut arr: [usize; NB_REPEATS]) -> Statistics {
    bubble_sort(&mut arr);

    let mut output: Statistics = Statistics {
        mean: 0,
        min: 0,
        max: 0,
        p25: 0,
        p50: 0,
        p75: 0,
        p95: 0,
        p99: 0,
    };

    output.min = arr[0];
    output.max = arr[arr.len() - 1];
    output.mean = arr.iter().sum::<usize>() / arr.len();

    let percentile = |per: f64| -> usize { arr[(per * arr.len() as f64) as usize] };

    output.p25 = percentile(0.25);
    output.p50 = percentile(0.50);
    output.p75 = percentile(0.75);
    output.p95 = percentile(0.95);
    output.p99 = percentile(0.99);

    output
}

fn print_statistics(stats: Statistics) {
    log::info!("{:?}", stats);
}
