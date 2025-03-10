// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
pub use confidential_memory_address::ConfidentialMemoryAddress;
pub use confidential_vm_physical_address::ConfidentialVmPhysicalAddress;
pub use non_confidential_memory_address::NonConfidentialMemoryAddress;
use pointers_utility::{ptr_align, ptr_byte_add_mut, ptr_byte_offset};
use spin::Once;

use crate::ace::core::architecture::PageSize;
use crate::ace::error::Error;
use crate::ensure;

mod confidential_memory_address;
mod confidential_vm_physical_address;
mod non_confidential_memory_address;

/// MEMORY_LAYOUT is a static variable (private to this module) that is set during the system boot and never changes
/// later -- this is guaranteed by Once<>. It stores an instance of the `MemoryLayout`. The only way to get a shared
/// access to this instance is by calling `MemoryLayout::read()` function.
static MEMORY_LAYOUT: Once<MemoryLayout> = Once::new();

/// Provides an interface to offset addresses that are guaranteed to remain inside the same memory region, i.e.,
/// confidential or non-confidential memory.
///
/// Model: A Coq `memory_layout` record containing the memory ranges for confidential and
/// non-confidential memory.
pub struct MemoryLayout {
    non_confidential_memory_start: *mut usize,
    non_confidential_memory_end: *const usize,
    confidential_memory_start: *mut usize,
    confidential_memory_end: *const usize,
}

/// Send+Sync are not automatically declared on the `MemoryLayout` type because it stores internally raw pointers that
/// are not safe to pass in a multi-threaded program. Declaring Send+Sync is safe because because we never expose raw
/// pointers outside the MemoryLayout except for the constructor that returns the initial address of the confidential
/// memory. The constructor is invoked only once by the initialization procedure during the boot of the system when the
/// system executes only on a one physical hart.
unsafe impl Send for MemoryLayout {}
unsafe impl Sync for MemoryLayout {}

impl MemoryLayout {
    const NOT_INITIALIZED_MEMORY_LAYOUT: &'static str =
        "Bug. Could not access MemoryLayout because is has not been initialized";

    /// Constructs the `MemoryLayout` where the confidential memory is within the memory range defined by
    /// `confidential_memory_start` and `confidential_memory_end`. Returns the `MemoryLayout` and the first alligned
    /// address in the confidential memory.
    ///
    /// # Safety
    ///
    /// This function must be called only once by the initialization procedure during the boot of the system.
    pub unsafe fn init(
        non_confidential_memory_start: *mut usize,
        non_confidential_memory_end: *const usize,
        confidential_memory_start: *mut usize,
        mut confidential_memory_end: *const usize,
    ) -> Result<(ConfidentialMemoryAddress, *const usize), Error> {
        assert!((non_confidential_memory_start as *const usize) < non_confidential_memory_end);
        assert!(non_confidential_memory_end <= (confidential_memory_start as *const usize));
        assert!((confidential_memory_start as *const usize) < confidential_memory_end);

        // We align the start of the confidential memory to the smallest possible page size (4KiB on RISC-V) and make
        // sure that its size is the multiply of this page size.
        let smalles_page_size_in_bytes = PageSize::smallest().in_bytes();
        let confidential_memory_start = ptr_align(
            confidential_memory_start,
            smalles_page_size_in_bytes,
            confidential_memory_end,
        )
        .map_err(|_| Error::NotEnoughMemory())?;
        // Let's make sure that the end of the confidential memory is properly aligned. I.e., there are no dangling
        // bytes after the last page.
        let memory_size = ptr_byte_offset(confidential_memory_end, confidential_memory_start);
        let memory_size = usize::try_from(memory_size).map_err(|_| Error::NotEnoughMemory())?;
        let number_of_pages = memory_size / smalles_page_size_in_bytes;
        let memory_size_in_bytes = number_of_pages * smalles_page_size_in_bytes;
        if memory_size > memory_size_in_bytes {
            // We must modify the end_address because the current one is not a multiply of the smallest page size
            confidential_memory_end = ptr_byte_add_mut(
                confidential_memory_start,
                memory_size_in_bytes,
                confidential_memory_end,
            )?;
        }

        MEMORY_LAYOUT.call_once(|| MemoryLayout {
            non_confidential_memory_start,
            non_confidential_memory_end,
            confidential_memory_start,
            confidential_memory_end,
        });

        Ok((
            ConfidentialMemoryAddress::new(confidential_memory_start),
            confidential_memory_end,
        ))
    }

    /// Offsets an address in the confidential memory by a given number of bytes. Returns an error if the resulting
    /// address is not in the confidential memory region.
    pub fn confidential_address_at_offset(
        &self,
        address: &ConfidentialMemoryAddress,
        offset_in_bytes: usize,
    ) -> Result<ConfidentialMemoryAddress, Error> {
        Ok(
            unsafe { address.add(offset_in_bytes, self.confidential_memory_end) }
                .map_err(|_| Error::AddressNotInConfidentialMemory())?,
        )
    }

    /// Offsets an address in the confidential memory by a given number of bytes. Returns an error if the resulting
    /// address is outside the confidential memory region or exceeds the given upper bound.
    pub fn confidential_address_at_offset_bounded(
        &self,
        address: &ConfidentialMemoryAddress,
        offset_in_bytes: usize,
        upper_bound: *const usize,
    ) -> Result<ConfidentialMemoryAddress, Error> {
        ensure!(
            upper_bound <= self.confidential_memory_end,
            Error::AddressNotInConfidentialMemory()
        )?;
        Ok(self.confidential_address_at_offset(address, offset_in_bytes)?)
    }

    /// Offsets an address in the non-confidential memory by given number of bytes. Returns an error if the resulting
    /// address is outside the non-confidential memory region.
    pub fn non_confidential_address_at_offset(
        &self,
        address: &NonConfidentialMemoryAddress,
        offset_in_bytes: usize,
    ) -> Result<NonConfidentialMemoryAddress, Error> {
        Ok(
            unsafe { address.add(offset_in_bytes, self.non_confidential_memory_end) }
                .map_err(|_| Error::AddressNotInNonConfidentialMemory())?,
        )
    }

    /// Returns true if the raw pointer is inside the non-confidential memory.
    pub fn is_in_non_confidential_range(&self, address: *const usize) -> bool {
        self.non_confidential_memory_start as *const usize <= address
            && address < self.non_confidential_memory_end
    }

    /// Clears all confidential memory, writting to it 0s.
    ///
    /// # Safety
    ///
    /// Caller must guarantee that there is no other thread that can write to confidential memory during execution of
    /// this function.
    // TODO(verification): we need to come up with a mechanism to acquire ownership of all memory
    // TODO: Add this in the panic handler of Miralis
    #[allow(dead_code)]
    pub unsafe fn clear_confidential_memory(&self) {
        // We can safely cast the below offset to usize because the constructor guarantees that the confidential memory
        // range is valid, and so the memory size must be a valid usize
        let memory_size =
            ptr_byte_offset(self.confidential_memory_end, self.confidential_memory_start) as usize;
        let usize_alligned_offsets = (0..memory_size).step_by(core::mem::size_of::<usize>());
        usize_alligned_offsets.for_each(|offset_in_bytes| {
            let _ = ptr_byte_add_mut(
                self.confidential_memory_start,
                offset_in_bytes,
                self.confidential_memory_end,
            )
            .and_then(|ptr| Ok(ptr.write_volatile(0)));
        });
    }

    /// Get a pointer to the globally initialized `MemoryLayout`.
    /// Panics if the memory layout has not been initialized yet.
    pub fn read() -> &'static MemoryLayout {
        MEMORY_LAYOUT
            .get()
            .expect(Self::NOT_INITIALIZED_MEMORY_LAYOUT)
    }

    /// Get the boundaries of confidential memory as a (start, end) tuple.
    pub fn confidential_memory_boundary(&self) -> (usize, usize) {
        (
            self.confidential_memory_start as usize,
            self.confidential_memory_end as usize,
        )
    }
}
