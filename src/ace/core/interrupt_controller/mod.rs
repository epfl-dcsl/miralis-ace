// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use spin::{Once, RwLock, RwLockReadGuard};

use crate::ace::core::architecture::CSR;
use crate::ace::error::Error;
use crate::ensure_not;
use crate::platform::{Plat, Platform};

const NOT_INITIALIZED_INTERRUPT_CONTROLLER: &str =
    "Bug. Could not access interrupt controller because it has not been initialized";

/// A static global structure for the interrupt controller. Once<> guarantees that it the interrupt controller can only
/// be initialized once.
static INTERRUPT_CONTROLLER: Once<RwLock<InterruptController>> = Once::new();

/*extern "C" {
    /// For now, we rely on the OpenSBI's functionality to send IPIs.
    fn sbi_ipi_send_smode(hmask: usize, hbase: usize) -> usize;
}*/

fn sbi_ipi_send_smode(_hmask: usize, _hbase: usize) -> usize {
    let mut clint = Plat::get_clint().lock();

    // TODO: Implement error handling here
    clint.write_msip(_hbase, _hmask as u32).unwrap();

    0
}

/// Interrupt controller abstract the functionality needed by the security monitor to interact with hart/device
/// interrupts.
pub struct InterruptController {}

impl<'a> InterruptController {
    /// Constructs the global, unique interrupt controller instance.
    pub fn initialize() -> Result<(), Error> {
        let interrupt_controller = Self::new()?;
        ensure_not!(
            INTERRUPT_CONTROLLER.is_completed(),
            Error::Reinitialization()
        )?;
        INTERRUPT_CONTROLLER.call_once(|| RwLock::new(interrupt_controller));
        Ok(())
    }

    fn new() -> Result<Self, Error> {
        // In future when we do not rely on OpenSBI, this function should parse the flatten device tree, detect type of the hardware
        // interrupt controller and take control over it.
        Ok(Self {})
    }

    pub fn send_ipi(&self, target_hart_id: usize) -> Result<(), Error> {
        // TODO: delay the IPI by random quant of time to prevent time-stepping the confidential VM.
        if target_hart_id == CSR.mhartid.read() {
            return Ok(());
        }
        let hart_mask = 1;
        let hart_mask_base = target_hart_id;
        // For now we rely on the underlying OpenSBI to send IPIs to hardware harts.
        match sbi_ipi_send_smode(hart_mask, hart_mask_base) {
            0 => Ok(()),
            code => Err(Error::InterruptSendingError(code)),
        }
    }

    pub fn try_read<F, O>(op: O) -> Result<F, Error>
    where
        O: FnOnce(&RwLockReadGuard<'_, InterruptController>) -> Result<F, Error>,
    {
        op(&INTERRUPT_CONTROLLER
            .get()
            .expect(NOT_INITIALIZED_INTERRUPT_CONTROLLER)
            .read())
    }
}
