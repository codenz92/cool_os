/// Virtual Memory Manager (Phase 10).
///
/// Provides a globally-accessible frame allocator and helpers for building and
/// switching per-process PML4 page tables.
///
/// Address-space layout:
///   L4 indices 0x00–0xFF (lower canonical half) — per-process user space
///   L4 indices 0x100–0x1FF (upper canonical half) — shared kernel mappings
///
/// Per-process user stacks are placed at USER_STACK_TOP (L4 index 0xFF),
/// chosen to sit in a canonical lower-half slot that the kernel never uses.
use spin::Mutex;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
        Size4KiB,
    },
    PhysAddr, VirtAddr,
};

use crate::memory::BootInfoFrameAllocator;

// ── Globals ───────────────────────────────────────────────────────────────────

static PHYS_OFFSET: Mutex<u64> = Mutex::new(0);
static FRAME_ALLOC: Mutex<Option<BootInfoFrameAllocator>> = Mutex::new(None);
static BOOT_PML4: Mutex<u64> = Mutex::new(0);

/// Per-process user stack: 64 KiB ending at this virtual address.
pub const USER_STACK_TOP: u64 = 0x0000_7fff_0010_0000;
/// Size of the per-process user stack (64 KiB).
pub const USER_STACK_SIZE: u64 = 64 * 1024;
/// Bottom of the user stack (guard page sits just below this).
pub const USER_STACK_BOTTOM: u64 = USER_STACK_TOP - USER_STACK_SIZE;

// ── Init ──────────────────────────────────────────────────────────────────────

pub fn init(phys_offset: VirtAddr, alloc: BootInfoFrameAllocator) {
    *PHYS_OFFSET.lock() = phys_offset.as_u64();
    *BOOT_PML4.lock() = Cr3::read().0.start_address().as_u64();
    *FRAME_ALLOC.lock() = Some(alloc);
}

// ── Internal helpers ──────────────────────────────────────────────────────────

pub fn phys_offset() -> VirtAddr {
    VirtAddr::new(*PHYS_OFFSET.lock())
}

/// Convert a physical address to its virtual alias via the physical-memory map.
pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    phys_offset() + phys.as_u64()
}

/// Borrow the physical frame at `phys` as a mutable `PageTable` reference.
///
/// # Safety
/// `phys` must be the start of a valid, 4 KiB-aligned page table frame.
unsafe fn table_at(phys: PhysAddr) -> &'static mut PageTable {
    &mut *(phys_to_virt(phys).as_mut_ptr())
}

/// Allocate one 4 KiB physical frame from the global allocator.
pub fn alloc_frame() -> Option<PhysFrame> {
    FRAME_ALLOC.lock().as_mut()?.allocate_frame()
}

/// Allocate a zeroed 4 KiB physical frame.
pub fn alloc_zeroed_frame() -> Option<PhysFrame> {
    let frame = alloc_frame()?;
    let ptr = phys_to_virt(frame.start_address()).as_mut_ptr::<u8>();
    unsafe { core::ptr::write_bytes(ptr, 0, 4096) };
    crate::slab::record_alloc("frames", 4096);
    Some(frame)
}

// ── Public VMM API ────────────────────────────────────────────────────────────

/// Allocate a new PML4, copying all kernel-half entries (L4 indices 256–511)
/// from the active PML4 so kernel code, heap, framebuffer, and physical-memory
/// map are reachable from the new address space.  Lower-half entries (0–255)
/// are left empty — user mappings live there.
pub fn new_process_pml4() -> Option<PhysFrame> {
    let frame = alloc_zeroed_frame()?;

    let boot_phys = PhysAddr::new(*BOOT_PML4.lock());
    let boot_l4_frame: PhysFrame = PhysFrame::containing_address(boot_phys);
    let src = unsafe { table_at(boot_l4_frame.start_address()) };
    let dst = unsafe { table_at(frame.start_address()) };

    // Copy all 512 L4 entries so the new PML4 has the same kernel mappings
    // (upper half: kernel .text, phys map) AND kernel lower-half allocations
    // (heap, scheduler stacks at L4 index 135, etc.).
    //
    // The user stack VA lives at L4 index 255, which is EMPTY in the boot
    // PML4.  Each process gets a fresh PDPT allocated under that entry by
    // map_region, so user stacks remain physically isolated despite all other
    // lower-half entries being shared by reference.
    for i in 0..512 {
        dst[i] = src[i].clone();
    }

    Some(frame)
}

/// Map `phys_frame` at virtual address `virt` inside the address space rooted
/// at `pml4_frame`, using the provided page-table flags.  Allocates intermediate
/// page-table frames as needed.
pub fn map_page_in(
    pml4_frame: PhysFrame,
    virt: VirtAddr,
    phys_frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let pml4 = unsafe { table_at(pml4_frame.start_address()) };
    let offset = phys_offset();
    let mut mapper = unsafe { OffsetPageTable::new(pml4, offset) };

    let page: Page<Size4KiB> = Page::containing_address(virt);

    let mut guard = FRAME_ALLOC.lock();
    let alloc = guard.as_mut().ok_or("frame allocator not initialized")?;

    unsafe {
        mapper
            .map_to(page, phys_frame, flags, alloc)
            .map_err(|_| "map_to failed")?
            .flush();
    }
    Ok(())
}

/// Map `len` bytes of freshly-allocated frames starting at `virt` inside the
/// address space rooted at `pml4_frame`.
pub fn map_region(
    pml4_frame: PhysFrame,
    virt: VirtAddr,
    len: u64,
    flags: PageTableFlags,
) -> Result<(), &'static str> {
    let mut offset = 0u64;
    while offset < len {
        let frame = alloc_zeroed_frame().ok_or("out of frames")?;
        map_page_in(pml4_frame, virt + offset, frame, flags)?;
        offset += 4096;
    }
    Ok(())
}

/// Load `pml4_frame` into CR3, switching to that address space.
///
/// # Safety
/// The PML4 must have valid kernel-half entries so that execution can continue
/// after the switch.  All currently-executing code must be reachable via the
/// new page table.
pub unsafe fn switch_to(pml4_frame: PhysFrame) {
    let (current, flags) = Cr3::read();
    if current != pml4_frame {
        Cr3::write(pml4_frame, flags);
    }
}

/// Switch back to the boot PML4 (used for kernel tasks with pml4=None).
///
/// # Safety
/// Same requirements as `switch_to`.
pub unsafe fn switch_to_boot() {
    let boot_phys = PhysAddr::new(*BOOT_PML4.lock());
    let frame = PhysFrame::containing_address(boot_phys);
    switch_to(frame);
}

/// Return the current PML4 physical frame.
pub fn current_pml4() -> PhysFrame {
    Cr3::read().0
}

pub fn user_range_accessible(ptr: u64, len: u64, writable: bool) -> bool {
    if ptr == 0 || len == 0 {
        return false;
    }
    let Some(last) = ptr.checked_add(len - 1) else {
        return false;
    };
    let mut page_addr = ptr & !0xfffu64;
    loop {
        if !user_page_accessible(VirtAddr::new(page_addr), writable) {
            return false;
        }
        if page_addr >= (last & !0xfffu64) {
            break;
        }
        page_addr = page_addr.saturating_add(4096);
    }
    true
}

fn user_page_accessible(virt: VirtAddr, writable: bool) -> bool {
    let page: Page<Size4KiB> = Page::containing_address(virt);
    let pml4 = unsafe { table_at(current_pml4().start_address()) };
    let l4 = &pml4[page.p4_index()];
    if !entry_allows(l4.flags(), writable) {
        return false;
    }
    let l3_table = unsafe { table_at(l4.addr()) };
    let l3 = &l3_table[page.p3_index()];
    if !entry_allows(l3.flags(), writable) {
        return false;
    }
    if l3.flags().contains(PageTableFlags::HUGE_PAGE) {
        return true;
    }
    let l2_table = unsafe { table_at(l3.addr()) };
    let l2 = &l2_table[page.p2_index()];
    if !entry_allows(l2.flags(), writable) {
        return false;
    }
    if l2.flags().contains(PageTableFlags::HUGE_PAGE) {
        return true;
    }
    let l1_table = unsafe { table_at(l2.addr()) };
    let l1 = &l1_table[page.p1_index()];
    entry_allows(l1.flags(), writable)
}

fn entry_allows(flags: PageTableFlags, writable: bool) -> bool {
    flags.contains(PageTableFlags::PRESENT)
        && flags.contains(PageTableFlags::USER_ACCESSIBLE)
        && (!writable || flags.contains(PageTableFlags::WRITABLE))
}
