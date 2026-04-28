/// GDT with ring-0 kernel segments, ring-3 user segments, and TSS (Phase 9).
///
/// Segment layout (index × 8 = byte offset):
///   0x00  null
///   0x08  kernel code (ring 0)   ← SYSCALL sets CS here
///   0x10  kernel data (ring 0)   ← SYSCALL sets SS here (= 0x08 + 8)
///   0x18  user data   (ring 3)   ← SYSRET sets SS here (= 0x10 + 8,  RPL=3)
///   0x20  user code   (ring 3)   ← SYSRET sets CS here (= 0x10 + 16, RPL=3)
///   0x28  TSS low
///   0x30  TSS high
///
/// STAR MSR: kernel‑CS‑base = 0x08, SYSRET‑base = 0x10.
use lazy_static::lazy_static;
use x86_64::{
    structures::{
        gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector},
        tss::TaskStateSegment,
    },
    VirtAddr,
};

// 64 KiB dedicated ring-0 stack used by the CPU on IRQ/exception entry from ring 3.
const ISR_STACK_SIZE: usize = 64 * 1024;
static mut ISR_STACK: [u8; ISR_STACK_SIZE] = [0; ISR_STACK_SIZE];

lazy_static! {
    static ref TSS: TaskStateSegment = {
        let mut tss = TaskStateSegment::new();
        // RSP0: kernel stack the CPU switches to on interrupt/exception from ring 3.
        tss.privilege_stack_table[0] = VirtAddr::new(
            core::ptr::addr_of!(ISR_STACK) as u64 + ISR_STACK_SIZE as u64
        );
        tss
    };

    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.add_entry(Descriptor::kernel_code_segment()); // 0x08
        let kernel_data = gdt.add_entry(Descriptor::kernel_data_segment()); // 0x10
        let user_data   = gdt.add_entry(Descriptor::user_data_segment());   // 0x18
        let user_code   = gdt.add_entry(Descriptor::user_code_segment());   // 0x20
        let tss_sel     = gdt.add_entry(Descriptor::tss_segment(&TSS));     // 0x28
        (gdt, Selectors { kernel_code, kernel_data, user_data, user_code, tss: tss_sel })
    };
}

struct Selectors {
    kernel_code: SegmentSelector,
    kernel_data: SegmentSelector,
    pub user_data: SegmentSelector,
    pub user_code: SegmentSelector,
    tss: SegmentSelector,
}

pub fn init() {
    use x86_64::instructions::segmentation::{Segment, CS, SS};
    use x86_64::instructions::tables::load_tss;

    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code);
        SS::set_reg(GDT.1.kernel_data);
        load_tss(GDT.1.tss);
    }
}

pub fn user_code_selector() -> SegmentSelector {
    GDT.1.user_code
}
pub fn user_data_selector() -> SegmentSelector {
    GDT.1.user_data
}
