use bootloader_api::info::{MemoryRegion, MemoryRegionKind};
use x86_64::{
    structures::paging::{
        FrameAllocator, OffsetPageTable, PageTable, PageTableFlags, PhysFrame, Size4KiB,
    },
    PhysAddr, VirtAddr,
};

pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;
    let (level_4_table_frame, _) = Cr3::read();
    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
    &mut *page_table_ptr
}

pub struct BootInfoFrameAllocator {
    memory_regions: &'static [MemoryRegion],
    next: usize,
}

impl BootInfoFrameAllocator {
    pub unsafe fn init(memory_regions: &'static [MemoryRegion]) -> Self {
        BootInfoFrameAllocator {
            memory_regions,
            next: 0,
        }
    }

    /// Start a new allocator that skips the first `start` usable frames,
    /// picking up exactly where a previous allocator with `next == start` left off.
    pub unsafe fn init_from(memory_regions: &'static [MemoryRegion], start: usize) -> Self {
        BootInfoFrameAllocator {
            memory_regions,
            next: start,
        }
    }

    /// How many frames have been allocated so far.
    pub fn next(&self) -> usize {
        self.next
    }

    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> + '_ {
        self.memory_regions
            .iter()
            .filter(|r| r.kind == MemoryRegionKind::Usable)
            .map(|r| r.start..r.end)
            .flat_map(|r| r.step_by(4096))
            .map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}

/// Mark every present PTE in the active page table as user-accessible (U/S=1).
///
/// Phase 9 runs a single-address-space model: the userspace stub lives in the
/// kernel binary and the user stack is a kernel static.  Making all pages
/// user-accessible lets ring-3 code execute and access data without a #PF.
/// Phase 10 will replace this with per-process page tables.
pub unsafe fn mark_all_user_accessible(phys_offset: VirtAddr) {
    use x86_64::registers::control::Cr3;

    let (l4_frame, _) = Cr3::read();
    let l4 = table_at(phys_offset, l4_frame.start_address());

    for l4e in l4.iter_mut() {
        if l4e.is_unused() {
            continue;
        }
        l4e.set_flags(l4e.flags() | PageTableFlags::USER_ACCESSIBLE);

        let l3 = table_at(phys_offset, l4e.addr());
        for l3e in l3.iter_mut() {
            if l3e.is_unused() {
                continue;
            }
            l3e.set_flags(l3e.flags() | PageTableFlags::USER_ACCESSIBLE);
            if l3e.flags().contains(PageTableFlags::HUGE_PAGE) {
                continue;
            }

            let l2 = table_at(phys_offset, l3e.addr());
            for l2e in l2.iter_mut() {
                if l2e.is_unused() {
                    continue;
                }
                l2e.set_flags(l2e.flags() | PageTableFlags::USER_ACCESSIBLE);
                if l2e.flags().contains(PageTableFlags::HUGE_PAGE) {
                    continue;
                }

                let l1 = table_at(phys_offset, l2e.addr());
                for l1e in l1.iter_mut() {
                    if l1e.is_unused() {
                        continue;
                    }
                    l1e.set_flags(l1e.flags() | PageTableFlags::USER_ACCESSIBLE);
                }
            }
        }
    }
    x86_64::instructions::tlb::flush_all();
}

unsafe fn table_at(phys_offset: VirtAddr, phys: PhysAddr) -> &'static mut PageTable {
    &mut *((phys_offset + phys.as_u64()).as_mut_ptr())
}
