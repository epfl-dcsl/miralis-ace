// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::architecture::riscv::specification::WFI_INSTRUCTION;
use crate::ace::core::control_data::ConfidentialHart;

/// Handles virtual instruction trap that occured during execution of the confidential hart.
pub struct VirtualInstruction {
    instruction: usize,
    instruction_length: usize,
}

impl VirtualInstruction {
    pub fn from_confidential_hart(confidential_hart: &ConfidentialHart) -> Self {
        // According to the RISC-V privilege spec, mtval should store virtual instruction
        let instruction = confidential_hart.csrs().mtval.read();
        let instruction_length = riscv_decode::instruction_length(instruction as u16);
        Self {
            instruction,
            instruction_length,
        }
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        let transformation = if self.instruction == WFI_INSTRUCTION {
            ApplyToConfidentialHart::VirtualInstruction(self)
        } else {
            // TODO: add support for some CSR manipulation
            // TODO: for not supported instructions, inject illegal instruction exception to the guest
            panic!("Not supported virtual instruction: {:x}", self.instruction);
        };
        confidential_flow.apply_and_exit_to_confidential_hart(transformation)
    }

    pub fn apply_to_confidential_hart(&self, confidential_hart: &mut ConfidentialHart) {
        confidential_hart
            .csrs_mut()
            .mepc
            .add(self.instruction_length);
    }
}
