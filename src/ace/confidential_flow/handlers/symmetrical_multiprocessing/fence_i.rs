// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::sbi::SbiResponse;
use crate::ace::confidential_flow::handlers::symmetrical_multiprocessing::Ipi;
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::control_data::{
    ConfidentialHart, ConfidentialHartRemoteCommand, ConfidentialHartRemoteCommandExecutable,
};

/// Handles a request from one confidential hart to execute fence.i instruction on remote confidential harts.
#[derive(Clone)]
pub struct RemoteFenceI {
    ipi: Ipi,
}

impl RemoteFenceI {
    pub fn from_confidential_hart(confidential_hart: &ConfidentialHart) -> Self {
        Self {
            ipi: Ipi::from_confidential_hart(confidential_hart),
        }
    }

    pub fn handle(self, mut confidential_flow: ConfidentialFlow) -> ! {
        let transformation = confidential_flow
            .broadcast_remote_command(ConfidentialHartRemoteCommand::RemoteFenceI(self))
            .and_then(|_| Ok(SbiResponse::success()))
            .unwrap_or_else(|error| SbiResponse::error(error));
        confidential_flow.apply_and_exit_to_confidential_hart(ApplyToConfidentialHart::SbiResponse(
            transformation,
        ))
    }
}

impl ConfidentialHartRemoteCommandExecutable for RemoteFenceI {
    fn execute_on_confidential_hart(&self, confidential_hart: &mut ConfidentialHart) {
        crate::ace::core::architecture::riscv::fence::fence_i();
        self.ipi.execute_on_confidential_hart(confidential_hart);
    }

    fn is_hart_selected(&self, hart_id: usize) -> bool {
        self.ipi.is_hart_selected(hart_id)
    }
}
