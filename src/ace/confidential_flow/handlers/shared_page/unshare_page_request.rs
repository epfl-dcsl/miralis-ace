// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::sbi::{SbiRequest, SbiResponse};
use crate::ace::confidential_flow::handlers::symmetrical_multiprocessing::RemoteHfenceGvmaVmid;
use crate::ace::confidential_flow::{ApplyToConfidentialHart, ConfidentialFlow};
use crate::ace::core::architecture::riscv::sbi::CovgExtension;
use crate::ace::core::architecture::{GeneralPurposeRegister, SharedPage};
use crate::ace::core::control_data::{
    ConfidentialHart, ConfidentialHartRemoteCommand, ConfidentialVmId, ControlDataStorage,
    ResumableOperation,
};
use crate::ace::core::memory_layout::ConfidentialVmPhysicalAddress;
use crate::ace::error::Error;
use crate::ace::non_confidential_flow::DeclassifyToHypervisor;
use crate::ensure;

/// Unshared memory that has been previously shared with the hypervisor.
pub struct UnsharePageRequest {
    address: ConfidentialVmPhysicalAddress,
    size: usize,
}

impl UnsharePageRequest {
    pub fn from_confidential_hart(confidential_hart: &ConfidentialHart) -> Self {
        Self {
            address: ConfidentialVmPhysicalAddress::new(
                confidential_hart.gprs().read(GeneralPurposeRegister::a0),
            ),
            size: confidential_hart.gprs().read(GeneralPurposeRegister::a1),
        }
    }

    pub fn handle(self, confidential_flow: ConfidentialFlow) -> ! {
        match self.unmap_shared_page(confidential_flow.confidential_vm_id()) {
            Ok(_) => confidential_flow
                .set_resumable_operation(ResumableOperation::SbiRequest())
                .into_non_confidential_flow()
                .declassify_and_exit_to_hypervisor(DeclassifyToHypervisor::SbiRequest(
                    self.unshare_page_sbi_request(),
                )),
            Err(error) => confidential_flow.apply_and_exit_to_confidential_hart(
                ApplyToConfidentialHart::SbiResponse(SbiResponse::error(error)),
            ),
        }
    }

    fn unshare_page_sbi_request(&self) -> SbiRequest {
        SbiRequest::new(
            CovgExtension::EXTID,
            CovgExtension::SBI_EXT_COVG_UNSHARE_MEMORY,
            self.address.usize(),
            self.size,
        )
    }

    fn unmap_shared_page(&self, confidential_vm_id: ConfidentialVmId) -> Result<(), Error> {
        ensure!(
            self.address.usize() % SharedPage::SIZE.in_bytes() == 0,
            Error::AddressNotAligned()
        )?;
        ensure!(
            self.size == SharedPage::SIZE.in_bytes(),
            Error::InvalidParameter()
        )?;

        ControlDataStorage::try_confidential_vm_mut(confidential_vm_id, |mut confidential_vm| {
            let unmapped_page_size = confidential_vm
                .memory_protector_mut()
                .unmap_shared_page(&self.address)?;
            let request = RemoteHfenceGvmaVmid::all_harts(
                &self.address,
                unmapped_page_size,
                confidential_vm_id,
            );
            confidential_vm.broadcast_remote_command(
                ConfidentialHartRemoteCommand::RemoteHfenceGvmaVmid(request),
            )?;
            Ok(())
        })
    }
}
