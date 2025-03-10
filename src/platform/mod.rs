mod miralis;
pub mod virt;
pub mod visionfive2;

use core::fmt;

use config_select::select_env;
use log::Level;
use spin::Mutex;

// Re-export virt platform by default for now
use crate::arch::{Arch, Architecture};
use crate::device::clint::VirtClint;
use crate::driver::ClintDriver;
use crate::{device, logger};

/// Export the current platform.
///
/// We use a custom proc macro that checks the value of an environment variable and select the
/// appropriate platform accordingly. This makes it possible to avoid adding an ever increasing set
/// of features and `#[cfg]` guards to select a platform.
pub type Plat = select_env!["MIRALIS_PLATFORM_NAME":
    "miralis"     => miralis::MiralisPlatform
    "visionfive2" => visionfive2::VisionFive2Platform
    _             => virt::VirtPlatform
];

pub trait Platform {
    fn name() -> &'static str;
    fn init();
    fn debug_print(level: Level, args: fmt::Arguments);
    fn exit_success() -> !;
    fn exit_failure() -> !;
    fn create_virtual_devices() -> [device::VirtDevice; 2];
    fn get_clint() -> &'static Mutex<ClintDriver>;
    fn get_vclint() -> &'static VirtClint;

    /// Signal a pending policy interrupt on all cores and trigger an MSI.
    ///
    /// As a result the policy interrupt callback will be called into on each cores.
    fn broadcast_policy_interrupt() {
        // Mark values in virtual clint
        Self::get_vclint().set_all_policy_msi();

        // Fire physical clint
        Self::get_clint().lock().trigger_msi_on_all_harts();
    }

    /// Load the firmware (virtual M-mode software) and return its address.
    fn load_firmware() -> usize;

    /// Returns the start and size of Miralis's own memory.
    fn get_miralis_memory_start_and_size() -> (usize, usize);

    /// Return maximum valid address
    fn get_max_valid_address() -> usize;

    const NB_HARTS: usize;
}

pub fn init() {
    Plat::init();
    logger::init();

    // Trap handler
    Arch::init();
}
