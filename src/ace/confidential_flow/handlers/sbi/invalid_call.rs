// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::sbi::SbiResponse;
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::architecture::GeneralPurposeRegister;
use crate::ace::core::control_data::ConfidentialHart;
use crate::ace::error::Error;
use crate::debug;

pub struct InvalidCall {
    extension_id: usize,
    function_id: usize,
}

impl InvalidCall {
    pub fn from_confidential_hart(confidential_hart: &ConfidentialHart) -> Self {
        Self {
            extension_id: confidential_hart.gprs().read(GeneralPurposeRegister::a7),
            function_id: confidential_hart.gprs().read(GeneralPurposeRegister::a6),
        }
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        debug!(
            "Not supported call {:x} {:x}",
            self.extension_id, self.function_id
        );
        let error = Error::InvalidCall(self.extension_id, self.function_id);
        let transformation = ApplyToConfidentialHart::SbiResponse(SbiResponse::error(error));
        confidential_flow.apply_and_exit_to_confidential_hart(transformation)
    }
}
