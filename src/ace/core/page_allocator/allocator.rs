// SPDX-FileCopyrightText: 2023 IBM Corporation
// SPDX-FileContributor: Wojciech Ozga <woz@zurich.ibm.com>, IBM Research - Zurich
// SPDX-License-Identifier: Apache-2.0
use alloc::vec;
use alloc::vec::Vec;

use spin::{Once, RwLock, RwLockWriteGuard};

use super::page::{Page, UnAllocated};
use crate::ace::core::architecture::PageSize;
use crate::ace::core::memory_layout::{ConfidentialMemoryAddress, MemoryLayout};
use crate::ace::error::Error;
use crate::{debug, ensure, ensure_not};

/// A static global structure containing unallocated pages. Once<> guarantees that the PageAllocator can only be initialized once.
static PAGE_ALLOCATOR: Once<RwLock<PageAllocator>> = Once::new();

/// This is a root node that represents the largest possible page size. Because of this implementation, there can be a maximum one page
/// token of the maximum size, and it will be then stored in the root node. It is reasonable as long as we do not support systems that have
/// more memory than 128TiB. For such systems, we must add larger page sizes.
/// Specification:
/// We give a "ghost name" γ to the allocator, which is used to link up page tokens allocated
/// with this allocator.

pub struct PageAllocator {
    base_address: usize,
    page_size: PageSize,
    root: PageStorageTreeNode,
}

impl PageAllocator {
    const NOT_INITIALIZED: &'static str = "Bug. Page allocator not initialized.";

    /// Initializes the global memory allocator with the given memory region as confidential memory. Must be called only once during the
    /// system initialization.
    ///
    /// # Arguments
    ///
    /// Both `memory_start` and `memory_end` must be aligned to 4KiB page boundaries.
    ///
    /// # Safety
    ///
    /// Caller must pass the ownership of the memory region [memory_start, memory_end).

    pub unsafe fn initialize(
        memory_start: ConfidentialMemoryAddress,
        memory_end: *const usize,
    ) -> Result<(), Error> {
        ensure_not!(PAGE_ALLOCATOR.is_completed(), Error::Reinitialization())?;
        let mut page_allocator = Self::empty();
        page_allocator.add_memory_region(memory_start, memory_end)?;
        // NOTE: We initialize the invariant here.
        PAGE_ALLOCATOR.call_once(|| RwLock::new(page_allocator));
        Ok(())
    }

    /// Specification:
    /// Postcondition: an initialized memory allocator is returned

    fn empty() -> Self {
        Self {
            root: PageStorageTreeNode::empty(),
            base_address: 0,
            page_size: PageSize::largest(),
        }
    }

    unsafe fn add_memory_region(
        &mut self,
        memory_region_start: ConfidentialMemoryAddress,
        memory_region_end: *const usize,
    ) -> Result<(), Error> {
        log::info!(
            "Memory tracker: adding memory region: 0x{:x} - 0x{:x}",
            memory_region_start.as_usize(),
            memory_region_end as usize
        );
        assert!(memory_region_start.is_aligned_to(PageSize::smallest().in_bytes()));
        assert!(memory_region_end.is_aligned_to(PageSize::smallest().in_bytes()));
        assert!(memory_region_start.as_usize() < memory_region_end as usize);
        // Page allocator supports maximum one page of largest size.
        ensure_not!(
            memory_region_start.offset_from(memory_region_end) > self.page_size.in_bytes() as isize,
            Error::TooMuchMemory()
        )?;

        // Our strategy is to create as few page tokens as possible to keep the memory overhead as low as possible. Therefore, we prefer to
        // create page tokens for the largest page size when possible. We use a greedy approach. We look for the largest possible page that
        // can be accomodated for the given address and create a page token for it. We start with the smallest possible page size and then
        // keep increasing it until we find the largest possible page size. Then, we keep decreasing the page size until we reach the end of
        // the memory region.
        let memory_layout = MemoryLayout::read();
        let mut memory_address = Some(memory_region_start);
        let mut page_size = PageSize::smallest();

        // We might have to create a few tokens of 4KiB until we reach the address at which we can fit a 2MiB page. Then, we might have to
        // create a few tokens for 2MiB pages until we get the address where 1 GiB page would fit. Consider the following example,
        // where we first create 7x 4 KiB tokens (++), then 3x 2 MiB tokens (**), and only then start creating 1 GiB tokens (##).
        //
        //      ++ ++ ++ ++ ++ ++ ++  ***********************  ***********************  ***********************  ####
        // ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  || ...
        //     ^memory_region_start  ^2 MiB                   ^2 MiB                   ^2 MiB                   ^1GiB
        //
        // At certain point we will not be able to fit more page tokens of the highest size (1GiB in our example) because remaining space
        // will be lower than the used page size. We might, however, still fit tokens of smaller sizes. This will be a analogous (but
        // opposite) situation to the one presented above. According to the following example, we will fit 3x 2 MiB (**) and 4x 4 KiB (++)
        // page tokens to the remaining memory region.
        //
        //   ***********************  ***********************  ***********************  ++ ++ ++ ++
        // ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  ||  |  |  |  |  |  |  |  || ...
        //  ^1 GiB                   ^2 MiB                   ^2 MiB                   ^2 MiB      ^memory_region_end

        // According to the RISC-V spec, pages must be aligned to their size.
        let is_address_page_aligned = |address: &ConfidentialMemoryAddress,
                                       page_size: &PageSize| {
            address.is_aligned_to(page_size.in_bytes())
        };

        // Page can be created only if all bytes are belonging to the given memory region
        let can_create_page = |address: &ConfidentialMemoryAddress, page_size: &PageSize| {
            let page_last_address = page_size.in_bytes() - 1;
            memory_layout
                .confidential_address_at_offset_bounded(
                    &address,
                    page_last_address,
                    memory_region_end,
                )
                .is_ok()
        };

        while let Some(address) = memory_address.take() {
            // Let's find the largest possible size of a page that could align to this address.
            while let Some(larger_size) = page_size
                .larger()
                .filter(|larger_size| is_address_page_aligned(&address, &larger_size))
            {
                page_size = larger_size;
            }
            // Now let's find the largest size of a page that really fits in the given memory region. We do not have to check the alignment,
            // because the larger pages sizes are multiplies of the smaller page sizes.
            while let Some(smaller_size) = page_size
                .smaller()
                .filter(|smaller_size| !can_create_page(&address, &smaller_size))
            {
                page_size = smaller_size;
            }
            // The following line ensures that the while loop will complete because, regardless of whether we manage to create a page token
            // or not, we will increment the `memory_address` in each loop so that it eventually passes the end of the given memory region.
            memory_address = memory_layout
                .confidential_address_at_offset_bounded(
                    &address,
                    page_size.in_bytes(),
                    memory_region_end,
                )
                .ok();
            // If the next memory address (`memory_address`) is still in the memory range, then we are sure we can create the page token.
            // Otherwise, we must check the boundary condition: Are we creating the last page token over a memory whose last byte
            // (`address`+`page_size.in_bytes()`) is next to the end of the memory region (`memory_region_end`)?
            if memory_address.is_some() || can_create_page(&address, &page_size) {
                let new_page_token = Page::<UnAllocated>::init(address, page_size.clone());
                // NOTE We show that the page token is within the range of
                // the allocator
                self.root
                    .store_page_token(self.base_address, self.page_size, new_page_token);
            }
        }
        Ok(())
    }

    /// Returns a page token that has ownership over an unallocated memory region of the requested size. Returns error if it could not
    /// obtain write access to the global instance of the page allocator or if there are not enough page tokens satisfying the requested
    /// criteria.
    /// Specification:
    pub fn acquire_page(page_size_to_allocate: PageSize) -> Result<Page<UnAllocated>, Error> {
        Self::try_write(|page_allocator| {
            let base_address = page_allocator.base_address;
            let page_size = page_allocator.page_size;
            Ok(page_allocator.root.acquire_page_token(
                base_address,
                page_size,
                page_size_to_allocate,
            ))
        })?
    }

    /// Consumes the page tokens given by the caller, allowing for their further acquisition. This is equivalent to deallocation of the
    /// physical memory region owned by the returned page tokens. Given vector of pages might contains pages of arbitrary sizes.
    pub fn release_pages(released_pages: Vec<Page<UnAllocated>>) {
        let _ = Self::try_write(|page_allocator| {
            let base_address = page_allocator.base_address;
            let page_size = page_allocator.page_size;
            released_pages.into_iter().for_each(|page_token| {
                // NOTE: we show that the token is within range of the allocator.
                page_allocator
                    .root
                    .store_page_token(base_address, page_size, page_token);
            });
            Ok(())
        })
        .inspect_err(|_| {
            debug!("Memory leak: failed to store released pages in the page allocator")
        });
    }

    /// returns a mutable reference to the PageAllocator after obtaining a lock on the mutex
    fn try_write<F, O>(op: O) -> Result<F, Error>
    where
        O: FnOnce(&mut RwLockWriteGuard<'static, PageAllocator>) -> Result<F, Error>,
    {
        op(&mut PAGE_ALLOCATOR.get().expect(Self::NOT_INITIALIZED).write())
    }
}

/// A node of a tree data structure that stores page tokens and maintains additional metadata that simplifies acquisition and
/// release of page tokens.
/// Specification:
/// A node is refined by the size of this node,
/// its base address,
/// and the logical allocation state.
// TODO: consider using separate public and private interfaces
struct PageStorageTreeNode {
    // Page token owned by this node. `None` means that this page token has already been allocated or that it has been divided into smaller
    // pages token that were stored in this node's children.
    page_token: Option<Page<UnAllocated>>,
    // Specifies what size of the page token can be allocated by exploring the tree starting at this node.
    // Invariant: if page token exist, then the its size is the max allocable size. Otherwise, the max allocable page size is the max
    // allocable page size of children
    max_allocable_page_size: Option<PageSize>,
    // Invariant: Children store page tokens smaller than the page token stored in the parent node
    children: Vec<Self>,
}

impl PageStorageTreeNode {
    /// Creates a new empty node with no allocation.
    /// Specification:
    /// We can choose an arbitrary node size and base address.
    /// Precondition: the base address needs to be suitably aligned.
    pub fn empty() -> Self {
        Self {
            page_token: None,
            max_allocable_page_size: None,
            children: vec![],
        }
    }

    /// Recursively traverses the tree until it reaches the node that can store the given page token. Returns the largest size of a page
    /// token that can be allocated from this node. This method has the max depth of recusrive invocation equal to the number of
    /// PageSize variants. This method has an upper bounded computation complexity.
    ///
    pub fn store_page_token(
        &mut self,
        this_node_base_address: usize,
        this_node_page_size: PageSize,
        page_token: Page<UnAllocated>,
    ) -> PageSize {
        assert!(this_node_page_size >= *page_token.size());
        if &this_node_page_size == page_token.size() {
            // End of recursion, we found the node that can store the page token.
            assert!(this_node_base_address == page_token.start_address());
            assert!(&this_node_page_size == page_token.size());
            assert!(self.page_token.is_none());
            self.store_page_token_in_this_node(page_token);
        } else {
            assert!(this_node_page_size.smaller().is_some());
            self.initialize_children_if_needed(this_node_page_size);

            // Calculate which child should we invoke recursively.
            let index = self.calculate_child_index(
                this_node_base_address,
                this_node_page_size,
                &page_token,
            );
            // Let's go recursively to the node where this page belongs to.
            let (child_base_address, child_page_size) =
                self.child_address_and_size(this_node_base_address, this_node_page_size, index);
            let allocable_page_size = self.children[index].store_page_token(
                child_base_address,
                child_page_size,
                page_token,
            );
            // We are coming back from the recursion.
            self.try_to_merge_page_tokens(this_node_page_size);
            if Some(allocable_page_size) > self.max_allocable_page_size {
                // Some children can allocate page tokens of a size larger than the page token we stored because they could have been
                // merged.
                self.max_allocable_page_size = Some(allocable_page_size);
            }
        }
        self.max_allocable_page_size.unwrap()
    }

    /// Recursively traverses the tree to reach a node that contains the page token of the requested size and returns this page token. This
    /// function returns also a set of page size that are not allocable anymore at the node. This method has the max depth of recusrive
    /// invocation equal to the number of PageSize variants. This method has an upper bounded computation complexity.
    ///
    /// Invariants: requested page size to acquire must not be greater than a page size allocable at this node.
    pub fn acquire_page_token(
        &mut self,
        this_node_base_address: usize,
        this_node_page_size: PageSize,
        page_size_to_acquire: PageSize,
    ) -> Result<Page<UnAllocated>, Error> {
        ensure!(
            self.max_allocable_page_size >= Some(page_size_to_acquire),
            Error::OutOfPages()
        )?;
        if &this_node_page_size == &page_size_to_acquire {
            // End of recursion, we found the node from which we acquire a page token.
            assert!(self.page_token.is_some());
            let page_token = self.acquire_page_token_from_this_node();
            assert!(this_node_base_address == page_token.start_address());
            assert!(&this_node_page_size == page_token.size());
            Ok(page_token)
        } else {
            // We are too high in the tree, i.e., the current level represents page size allocations that are larger than the page size
            // that was requested.
            assert!(this_node_page_size.smaller().is_some());
            // Lazily initialize children
            self.initialize_children_if_needed(this_node_page_size);

            // Because we are looking for a page token of a smaller size, we must divide the page token owned by this node, if that has
            // not yet occured.
            if self.page_token.is_some() {
                self.divide_page_token(this_node_base_address, this_node_page_size);
            }
            // Let's get the index of the first child that has requested allocation. It might be that there is no child that can has the
            // required allocation. In such a case, we return an error.
            let index = self
                .children
                .iter()
                .position(|node| node.max_allocable_page_size >= Some(page_size_to_acquire))
                .ok_or(Error::OutOfPages())?;
            let (child_base_address, child_page_size) =
                self.child_address_and_size(this_node_base_address, this_node_page_size, index);
            // Invoke recursively this function to traverse to a node containing a page token of the requested size.
            // The below unwrap is ok because we found an index of a node that has requested allocation.
            let page_token = self.children[index]
                .acquire_page_token(child_base_address, child_page_size, page_size_to_acquire)
                .unwrap();
            // Let's refresh information about the largest allocable page size available in children.
            self.max_allocable_page_size = self
                .children
                .iter()
                .map(|child| child.max_allocable_page_size)
                .max()
                .flatten();
            Ok(page_token)
        }
    }

    /// Creates children for the given node because the node gets created with an empty list of children, expecting that children will be
    /// created lazily with this function.
    fn initialize_children_if_needed(&mut self, this_node_page_size: PageSize) {
        if self.children.is_empty() {
            self.children = (0..this_node_page_size.number_of_smaller_pages())
                .map(|_| Self::empty())
                .collect();
        }
    }

    /// Stores the given page token in the current node.
    fn store_page_token_in_this_node(&mut self, page_token: Page<UnAllocated>) {
        assert!(self.page_token.is_none());
        self.max_allocable_page_size = Some(*page_token.size());
        self.page_token = Some(page_token);
    }

    /// Returns an ownership of a page token that has been stored in this node.
    fn acquire_page_token_from_this_node(&mut self) -> Page<UnAllocated> {
        assert!(self.page_token.is_some());
        self.max_allocable_page_size = None;
        self.page_token.take().unwrap()
    }

    /// Divides the page token into smaller page tokens.
    fn divide_page_token(&mut self, this_node_base_address: usize, this_node_page_size: PageSize) {
        assert!(self.page_token.is_some());

        let mut smaller_pages = self.page_token.take().unwrap().divide();
        assert!(smaller_pages.len() == self.children.len());
        smaller_pages.drain(..).for_each(|smaller_page_token| {
            let index = self.calculate_child_index(
                this_node_base_address,
                this_node_page_size,
                &smaller_page_token,
            );
            self.children[index].store_page_token_in_this_node(smaller_page_token);
        });
        let smaller_size = self.max_allocable_page_size.unwrap().smaller().unwrap();
        self.max_allocable_page_size = Some(smaller_size);
    }

    /// Merges page tokens owned by children.
    fn try_to_merge_page_tokens(&mut self, this_node_page_size: PageSize) {
        if self.children.iter().all(|child| child.page_token.is_some()) {
            // All children have page tokens, thus we can merge them.
            let pages_to_merge = self
                .children
                .iter_mut()
                .map(|child| child.acquire_page_token_from_this_node())
                .collect();
            self.store_page_token_in_this_node(unsafe {
                Page::merge(pages_to_merge, this_node_page_size)
            });
            self.max_allocable_page_size = Some(this_node_page_size);
        }
    }

    /// Returns the index of a child that can store the page token.
    // TODO: the token is in the range of the child node.
    // TODO: does not work at this level of abstraction. Use a raw specification.
    fn calculate_child_index(
        &self,
        this_node_base_address: usize,
        this_node_page_size: PageSize,
        page_token: &Page<UnAllocated>,
    ) -> usize {
        assert!(this_node_page_size > *page_token.size());
        let index = (page_token.start_address() - this_node_base_address)
            / this_node_page_size.smaller().unwrap().in_bytes();
        assert!(index < self.children.len());
        index
    }

    /// Returns the base address and the page size of the child at the given index
    fn child_address_and_size(
        &self,
        this_node_base_address: usize,
        this_node_page_size: PageSize,
        index: usize,
    ) -> (usize, PageSize) {
        assert!(index < self.children.len());
        assert!(this_node_page_size.smaller().is_some());
        let child_base_address =
            this_node_base_address + index * this_node_page_size.smaller().unwrap().in_bytes();
        let child_page_size = this_node_page_size.smaller().unwrap();
        (child_base_address, child_page_size)
    }
}
