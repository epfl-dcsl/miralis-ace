#![no_std]
#![no_main]

mod arch;
mod decoder;
mod logger;
mod platform;
mod virt;

use core::panic::PanicInfo;

use arch::{pmpcfg, Arch, Architecture};
use platform::{init, Plat, Platform};

use crate::arch::{Csr, MCause, Register};
use crate::decoder::{decode, Instr};
use crate::virt::VirtContext;

// Defined in the linker script
extern "C" {
    pub(crate) static _stack_start: usize;
    pub(crate) static _stack_end: usize;
}

pub(crate) extern "C" fn main(hart_id: usize, device_tree_blob_addr: usize) -> ! {
    init();
    log::info!("Hello, world!");
    log::info!("Hart ID: {}", hart_id);
    log::info!("mstatus: 0x{:x}", Arch::read_mstatus());
    log::info!("DTS address: 0x{:x}", device_tree_blob_addr);

    log::info!("Preparing jump into payload");
    let payload_addr = Plat::load_payload();
    let mut ctx = VirtContext::default();
    unsafe {
        // Set return address, mode and PMP permissions
        Arch::write_mepc(payload_addr);
        Arch::set_mpp(arch::Mode::U);
        Arch::write_pmpcfg(0, pmpcfg::R | pmpcfg::W | pmpcfg::X | pmpcfg::TOR);
        Arch::write_pmpaddr(0, usize::MAX);
        // Configure the payload context
        ctx[Register::X2] = Plat::payload_stack_address();
        ctx[Register::X10] = hart_id;
        ctx[Register::X11] = device_tree_blob_addr;
    }

    main_loop(ctx);
}

fn main_loop(mut ctx: VirtContext) -> ! {
    loop {
        unsafe {
            Arch::enter_virt_firmware(&mut ctx);
            handle_trap(&mut ctx);
            log::info!("{:x?}", &ctx);
        }
    }
}

fn handle_trap(ctx: &mut VirtContext) {
    log::info!("Trapped!");
    log::info!("  mcause:  {:?}", Arch::read_mcause());
    log::info!("  mstatus: 0x{:x}", Arch::read_mstatus());
    log::info!("  mepc:    0x{:x}", Arch::read_mepc());
    log::info!("  mtval:   0x{:x}", Arch::read_mtval());

    // Temporary safeguard
    unsafe {
        static mut TRAP_COUNTER: usize = 0;

        if TRAP_COUNTER >= 100 {
            log::error!("Trap counter reached limit");
            Plat::exit_failure();
        }
        TRAP_COUNTER += 1;
    }

    match Arch::read_mcause() {
        MCause::EcallFromMMode | MCause::EcallFromUMode => {
            // For now we just exit successfuly
            log::info!("Success!");
            Plat::exit_success();
        }
        MCause::IllegalInstr => {
            let instr = unsafe { Arch::get_raw_faulting_instr() };
            let instr = decode(instr);
            log::info!("Faulting instruction: {:?}", instr);
            emulate_instr(ctx, &instr);
        }
        _ => (), // Continue
    }

    // Skip instruction and return
    unsafe {
        log::info!("Skipping trapping instruction");
        Arch::write_mepc(Arch::read_mepc() + 4);
    }
}

fn emulate_instr(ctx: &mut VirtContext, instr: &Instr) {
    match instr {
        Instr::Wfi => {
            // For now payloads only call WFI when panicking
            log::error!("Payload panicked!");
            Plat::exit_failure();
        }
        Instr::Csrrw { csr, rd, rs1 } => {
            if csr.is_unknown() {
                todo!("Unknown CSR");
            }
            let tmp = ctx[*csr];
            ctx[*csr] = ctx[*rs1];
            ctx[*rd] = tmp;
            csr_side_effect(ctx, *csr);
        }
        Instr::Csrrs { csr, rd, rs1 } => {
            if csr.is_unknown() {
                todo!("Unknown CSR");
            }
            let tmp = ctx[*csr];
            ctx[*csr] = tmp | ctx[*rs1];
            ctx[*rd] = tmp;
            csr_side_effect(ctx, *csr);
        }
        Instr::Csrrwi { csr, rd, uimm } => {
            if csr.is_unknown() {
                todo!("Unknown CSR");
            }
            ctx[*rd] = ctx[*csr];
            ctx[*csr] = *uimm;
            csr_side_effect(ctx, *csr);
        }
        _ => todo!("Instruction not yet implemented: {:?}", instr),
    }
}

/// Some CSRs might have side effect when written, this functions emulate those side effects.
fn csr_side_effect(_ctx: &mut VirtContext, csr: Csr) {
    match csr {
        Csr::Mstatus => todo!("Emulate mstatus"),
        Csr::Mie => (),      // No side effect
        Csr::Mtvec => (),    // No side effect
        Csr::Mscratch => (), // No side effect
        Csr::Unknown => panic!("Unknown CSR"),
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    log::error!("Panicked at {:#?} ", info);
    Plat::exit_failure();
}
