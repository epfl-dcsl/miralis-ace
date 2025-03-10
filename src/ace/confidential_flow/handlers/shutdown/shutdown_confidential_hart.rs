// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use crate::ace::confidential_flow::handlers::sbi::SbiRequest;
use crate::ace::confidential_flow::ConfidentialFlow;
use crate::ace::core::architecture::riscv::sbi::SrstExtension;
use crate::ace::core::control_data::ControlDataStorage;
use crate::ace::non_confidential_flow::DeclassifyToHypervisor;

/// Shuts down the currently executing confidential hart (and the corresponding confidential VM, if possible).
/// After cleaning up, this functions passes the control to the hypervisor informing it that the confidential VM has
/// been shutdown.
///
/// Always returns the control flow to the hypervisor informing it about the shutdown of the confidential VM.
pub fn shutdown_confidential_hart(mut confidential_flow: ConfidentialFlow) -> ! {
    let confidential_vm_id = confidential_flow.confidential_vm_id();
    // change the lifecycle status of the confidential hart to Shutdown
    confidential_flow.shutdown_confidential_hart();
    // The procedure of removing the confidential VM from the control data must be handled in the non-confidential flow
    // because all confidential harts must be released back to the control data.
    let non_confidential_flow = confidential_flow.into_non_confidential_flow();
    let _ = ControlDataStorage::remove_confidential_vm(confidential_vm_id);
    // We ignore the result of removing the confidential vm from the control data because it will return an error as
    // long as all confidential harts are in the `Shutdown` state. We do not know which confidential hart will be the
    // last one to shutdown, so we always try to remove the confidential VM when a confidential hart goes through the
    // shutdown procedure.
    let kvm_srst_system_reset =
        SbiRequest::new(SrstExtension::EXTID, SrstExtension::SYSTEM_RESET_FID, 0, 0);
    non_confidential_flow.declassify_and_exit_to_hypervisor(DeclassifyToHypervisor::SbiRequest(
        kvm_srst_system_reset,
    ))
}
