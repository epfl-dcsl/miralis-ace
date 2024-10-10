// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::sbi::SbiResponse;
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::architecture::CSR;
use crate::ace::core::control_data::ConfidentialHart;

pub struct SbiGetMvendorid {}

impl SbiGetMvendorid {
    pub fn from_confidential_hart(_: &ConfidentialHart) -> Self {
        Self {}
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        let mvendorid = CSR.mvendorid.read();
        let transformation =
            ApplyToConfidentialHart::SbiResponse(SbiResponse::success_with_code(mvendorid));
        confidential_flow.apply_and_exit_to_confidential_hart(transformation)
    }
}
