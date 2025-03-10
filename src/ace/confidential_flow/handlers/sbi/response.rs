// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::{ConfidentialFlow, DeclassifyToConfidentialVm};
use crate::ace::core::architecture::riscv::sbi::SBI_SUCCESS;
use crate::ace::core::architecture::riscv::specification::ECALL_INSTRUCTION_LENGTH;
use crate::ace::core::architecture::GeneralPurposeRegister;
use crate::ace::core::control_data::{ConfidentialHart, HypervisorHart};
use crate::ace::error::Error;

pub struct SbiResponse {
    a0: usize,
    a1: usize,
}

impl SbiResponse {
    pub fn from_hypervisor_hart(hypervisor_hart: &HypervisorHart) -> Self {
        Self {
            a0: hypervisor_hart.gprs().read(GeneralPurposeRegister::a0),
            a1: hypervisor_hart.gprs().read(GeneralPurposeRegister::a1),
        }
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        confidential_flow
            .declassify_and_exit_to_confidential_hart(DeclassifyToConfidentialVm::SbiResponse(self))
    }

    pub fn declassify_to_confidential_hart(&self, confidential_hart: &mut ConfidentialHart) {
        self.apply_to_confidential_hart(confidential_hart);
    }

    pub fn apply_to_confidential_hart(&self, confidential_hart: &mut ConfidentialHart) {
        confidential_hart
            .gprs_mut()
            .write(GeneralPurposeRegister::a0, self.a0);
        confidential_hart
            .gprs_mut()
            .write(GeneralPurposeRegister::a1, self.a1);
        confidential_hart
            .csrs_mut()
            .mepc
            .add(ECALL_INSTRUCTION_LENGTH);
    }

    pub fn declassify_to_hypervisor_hart(&self, hypervisor_hart: &mut HypervisorHart) {
        hypervisor_hart
            .gprs_mut()
            .write(GeneralPurposeRegister::a0, self.a0);
        hypervisor_hart
            .gprs_mut()
            .write(GeneralPurposeRegister::a1, self.a1);
        hypervisor_hart
            .csrs_mut()
            .mepc
            .add(ECALL_INSTRUCTION_LENGTH);
    }

    pub fn success() -> Self {
        Self::success_with_code(0)
    }

    pub fn success_with_code(code: usize) -> Self {
        Self {
            a0: SBI_SUCCESS as usize,
            a1: code,
        }
    }

    pub fn error(error: Error) -> Self {
        Self {
            a0: error.sbi_error_code(),
            a1: 0,
        }
    }
}
