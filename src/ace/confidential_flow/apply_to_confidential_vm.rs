// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::mmio::MmioAccessFault;
use crate::ace::confidential_flow::handlers::sbi::SbiResponse;
use crate::ace::confidential_flow::handlers::virtual_instructions::VirtualInstruction;

/// Transformation of the confidential hart state in a response to processing of a confidential hart call.
pub enum ApplyToConfidentialHart {
    MmioAccessFault(MmioAccessFault),
    SbiResponse(SbiResponse),
    VirtualInstruction(VirtualInstruction),
}
