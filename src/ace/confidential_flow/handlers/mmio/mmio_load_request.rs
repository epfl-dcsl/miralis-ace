// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::mmio::{MmioAccessFault, MmioLoadPending};
use crate::ace::confidential_flow::handlers::sbi::SbiResponse;
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::architecture::is_bit_enabled;
use crate::ace::core::architecture::specification::CAUSE_LOAD_ACCESS;
use crate::ace::core::control_data::{ConfidentialHart, HypervisorHart, ResumableOperation};
use crate::ace::non_confidential_flow::DeclassifyToHypervisor;

/// Handles MMIO load request coming from the confidential hart. This request will be declassified to the hypervisor.
pub struct MmioLoadRequest {
    mcause: usize,
    mtval: usize,
    mtval2: usize,
    mtinst: usize,
}

impl MmioLoadRequest {
    pub fn from_confidential_hart(confidential_hart: &ConfidentialHart) -> Self {
        Self {
            mcause: confidential_hart.csrs().mcause.read(),
            mtval: confidential_hart.csrs().mtval.read(),
            mtval2: confidential_hart.csrs().mtval2.read(),
            mtinst: confidential_hart.csrs().mtinst.read(),
        }
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        // According to the RISC-V privilege spec, mtinst encodes faulted instruction (bit 0 is 1) or a pseudo instruction
        assert!(self.mtinst & 0x1 > 0);
        let instruction = self.mtinst | 0x3;
        let instruction_length = if is_bit_enabled(self.mtinst, 1) {
            riscv_decode::instruction_length(instruction as u16)
        } else {
            2
        };

        let fault_address = (self.mtval2 << 2) | (self.mtval & 0x3);
        if !MmioAccessFault::tried_to_access_valid_mmio_region(
            confidential_flow.confidential_vm_id(),
            fault_address,
        ) {
            let mmio_access_fault_handler =
                MmioAccessFault::new(CAUSE_LOAD_ACCESS.into(), self.mtval, instruction_length);
            confidential_flow.apply_and_exit_to_confidential_hart(
                ApplyToConfidentialHart::MmioAccessFault(mmio_access_fault_handler),
            );
        }

        match crate::ace::core::architecture::decode_result_register(instruction) {
            Ok(gpr) => confidential_flow
                .set_resumable_operation(ResumableOperation::MmioLoad(MmioLoadPending::new(
                    instruction_length,
                    gpr,
                )))
                .into_non_confidential_flow()
                .declassify_and_exit_to_hypervisor(DeclassifyToHypervisor::MmioLoadRequest(self)),
            Err(error) => {
                let transformation = DeclassifyToHypervisor::SbiResponse(SbiResponse::error(error));
                confidential_flow
                    .into_non_confidential_flow()
                    .declassify_and_exit_to_hypervisor(transformation)
            }
        }
    }

    pub fn declassify_to_hypervisor_hart(&self, hypervisor_hart: &mut HypervisorHart) {
        use crate::ace::core::architecture::riscv::specification::*;
        // The security monitor exposes `scause` and `stval` via hart's CSRs but `htval` and `htinst` via the NACL shared memory.
        hypervisor_hart.csrs_mut().scause.write(self.mcause);
        hypervisor_hart.csrs_mut().stval.write(self.mtval);
        hypervisor_hart
            .shared_memory_mut()
            .write_csr(CSR_HTVAL.into(), self.mtval2);
        hypervisor_hart
            .shared_memory_mut()
            .write_csr(CSR_HTINST.into(), self.mtinst);
        SbiResponse::success().declassify_to_hypervisor_hart(hypervisor_hart);
    }
}
