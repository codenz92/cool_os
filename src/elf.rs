/// Minimal ELF64 loader for user processes.
///
/// Supports statically linked ELF64 binaries with page-aligned PT_LOAD segments
/// in the lower canonical half. The loader can either spawn a fresh task or
/// prepare an image for `sys_exec` to install into the current task.
extern crate alloc;
use alloc::vec::Vec;
use core::{cmp, mem, ptr};

use x86_64::{
    structures::paging::{PageTableFlags, PhysFrame},
    VirtAddr,
};

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u32 = 1;
const ET_EXEC: u16 = 2;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PF_W: u32 = 1 << 1;
const PAGE_SIZE: u64 = 4096;
const USER_CANONICAL_LIMIT: u64 = 0x0000_8000_0000_0000;

#[derive(Clone, Copy)]
#[repr(C)]
struct Elf64Header {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Elf64ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

pub enum ExecError {
    NotFound,
    InvalidElf(&'static str),
    OutOfMemory,
    MapFailed(&'static str),
    FdInstallFailed,
}

impl ExecError {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecError::NotFound => "file not found",
            ExecError::InvalidElf(msg) => msg,
            ExecError::OutOfMemory => "out of memory",
            ExecError::MapFailed(msg) => msg,
            ExecError::FdInstallFailed => "fd install failed",
        }
    }
}

pub struct LoadedImage {
    pub pml4: PhysFrame,
    pub entry: u64,
    pub user_rsp: u64,
}

#[allow(dead_code)]
pub fn spawn_elf_process(path: &str) -> Result<(), ExecError> {
    spawn_elf_process_with_args(path, &[])
}

pub fn spawn_elf_process_with_args(path: &str, args: &[&str]) -> Result<(), ExecError> {
    spawn_elf_process_with_fds(path, args, &[])
}

pub fn spawn_elf_process_with_fds(
    path: &str,
    args: &[&str],
    inherited_fds: &[(usize, usize)],
) -> Result<(), ExecError> {
    let image = load_elf_image_with_args(path, args)?;

    let ok = x86_64::instructions::interrupts::without_interrupts(|| {
        crate::scheduler::SCHEDULER.lock().spawn_user_with_fds(
            "user-elf",
            image.entry,
            image.user_rsp,
            image.pml4,
            inherited_fds,
        )
    });

    if ok {
        Ok(())
    } else {
        Err(ExecError::FdInstallFailed)
    }
}

pub fn spawn_elf_process_with_stdin(
    path: &str,
    args: &[&str],
    stdin_fd: usize,
) -> Result<(), ExecError> {
    spawn_elf_process_with_fds(path, args, &[(stdin_fd, 3)])
}

pub fn load_elf_image(path: &str) -> Result<LoadedImage, ExecError> {
    load_elf_image_with_args(path, &[])
}

pub fn load_elf_image_with_args(path: &str, args: &[&str]) -> Result<LoadedImage, ExecError> {
    let image = crate::fat32::read_file(path).ok_or(ExecError::NotFound)?;
    let header = parse_header(&image)?;
    let pml4 = crate::vmm::new_process_pml4().ok_or(ExecError::OutOfMemory)?;

    let user_rsp = map_user_stack(pml4, path, args)?;
    load_segments(&image, &header, pml4)?;

    Ok(LoadedImage {
        pml4,
        entry: header.e_entry,
        user_rsp,
    })
}

fn parse_header(image: &[u8]) -> Result<Elf64Header, ExecError> {
    let header = read_struct::<Elf64Header>(image, 0)
        .ok_or(ExecError::InvalidElf("truncated ELF header"))?;

    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ExecError::InvalidElf("bad ELF magic"));
    }
    if header.e_ident[4] != ELFCLASS64 {
        return Err(ExecError::InvalidElf("expected ELF64"));
    }
    if header.e_ident[5] != ELFDATA2LSB {
        return Err(ExecError::InvalidElf("expected little-endian ELF"));
    }
    if header.e_type != ET_EXEC {
        return Err(ExecError::InvalidElf("expected ET_EXEC"));
    }
    if header.e_machine != EM_X86_64 {
        return Err(ExecError::InvalidElf("expected x86_64 ELF"));
    }
    if header.e_version != EV_CURRENT {
        return Err(ExecError::InvalidElf("unsupported ELF version"));
    }
    if header.e_phentsize as usize != mem::size_of::<Elf64ProgramHeader>() {
        return Err(ExecError::InvalidElf("unexpected program header size"));
    }
    if header.e_entry >= USER_CANONICAL_LIMIT {
        return Err(ExecError::InvalidElf("entry point must be in user space"));
    }

    let ph_table_end = header
        .e_phoff
        .checked_add(header.e_phentsize as u64 * header.e_phnum as u64)
        .ok_or(ExecError::InvalidElf("program header table overflow"))?;
    if ph_table_end > image.len() as u64 {
        return Err(ExecError::InvalidElf("truncated program header table"));
    }

    Ok(header)
}

fn map_user_stack(pml4: PhysFrame, argv0: &str, args: &[&str]) -> Result<u64, ExecError> {
    let stack_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
    let mut top_frame = None;
    let mut offset = 0u64;
    while offset < crate::vmm::USER_STACK_SIZE {
        let frame = crate::vmm::alloc_zeroed_frame().ok_or(ExecError::OutOfMemory)?;
        let virt = VirtAddr::new(crate::vmm::USER_STACK_BOTTOM + offset);
        crate::vmm::map_page_in(pml4, virt, frame, stack_flags).map_err(ExecError::MapFailed)?;
        if offset + PAGE_SIZE == crate::vmm::USER_STACK_SIZE {
            top_frame = Some(frame);
        }
        offset += PAGE_SIZE;
    }

    let guard_addr = VirtAddr::new(crate::vmm::USER_STACK_BOTTOM - PAGE_SIZE);
    let guard_frame = crate::vmm::alloc_zeroed_frame().ok_or(ExecError::OutOfMemory)?;
    crate::vmm::map_page_in(pml4, guard_addr, guard_frame, PageTableFlags::PRESENT)
        .map_err(ExecError::MapFailed)?;

    let top_frame = top_frame.ok_or(ExecError::OutOfMemory)?;
    build_initial_stack(top_frame, argv0, args)
}

fn build_initial_stack(top_frame: PhysFrame, argv0: &str, args: &[&str]) -> Result<u64, ExecError> {
    let stack_page_base = crate::vmm::USER_STACK_TOP - PAGE_SIZE;
    let page = crate::vmm::phys_to_virt(top_frame.start_address()).as_mut_ptr::<u8>();

    let mut argv: Vec<&str> = Vec::with_capacity(args.len() + 1);
    argv.push(argv0);
    argv.extend_from_slice(args);

    let strings_bytes = argv.iter().map(|arg| arg.len() + 1).sum::<usize>();
    let pointer_bytes = (argv.len() + 3) * 8; // argc + argv[] + null + envp null
    if strings_bytes + pointer_bytes > PAGE_SIZE as usize {
        return Err(ExecError::InvalidElf(
            "argv too large for initial stack page",
        ));
    }

    let mut rsp = crate::vmm::USER_STACK_TOP;
    let mut argv_ptrs = [0u64; 8];
    if argv.len() > argv_ptrs.len() {
        return Err(ExecError::InvalidElf("too many argv entries"));
    }

    for (idx, arg) in argv.iter().enumerate().rev() {
        let bytes = arg.as_bytes();
        rsp -= (bytes.len() + 1) as u64;
        argv_ptrs[idx] = rsp;
        write_bytes(page, stack_page_base, rsp, bytes);
        write_byte(page, stack_page_base, rsp + bytes.len() as u64, 0);
    }

    rsp &= !0xf;
    rsp -= 8;
    write_u64(page, stack_page_base, rsp, 0); // envp[0] = NULL
    rsp -= 8;
    write_u64(page, stack_page_base, rsp, 0); // argv[argc] = NULL
    for idx in (0..argv.len()).rev() {
        rsp -= 8;
        write_u64(page, stack_page_base, rsp, argv_ptrs[idx]);
    }
    rsp -= 8;
    write_u64(page, stack_page_base, rsp, argv.len() as u64); // argc

    Ok(rsp)
}

fn load_segments(image: &[u8], header: &Elf64Header, pml4: PhysFrame) -> Result<(), ExecError> {
    let mut saw_load = false;

    for i in 0..header.e_phnum {
        let off = header.e_phoff as usize + i as usize * header.e_phentsize as usize;
        let ph = read_struct::<Elf64ProgramHeader>(image, off)
            .ok_or(ExecError::InvalidElf("truncated program header"))?;
        if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
            continue;
        }
        saw_load = true;
        validate_load_segment(&ph, image.len())?;
        map_load_segment(image, &ph, pml4)?;
    }

    if !saw_load {
        return Err(ExecError::InvalidElf("ELF has no PT_LOAD segments"));
    }

    Ok(())
}

fn validate_load_segment(ph: &Elf64ProgramHeader, image_len: usize) -> Result<(), ExecError> {
    if ph.p_filesz > ph.p_memsz {
        return Err(ExecError::InvalidElf("PT_LOAD filesz exceeds memsz"));
    }
    if ph.p_vaddr >= USER_CANONICAL_LIMIT {
        return Err(ExecError::InvalidElf(
            "PT_LOAD address must be in user space",
        ));
    }
    let file_end = ph
        .p_offset
        .checked_add(ph.p_filesz)
        .ok_or(ExecError::InvalidElf("PT_LOAD file range overflow"))?;
    if file_end > image_len as u64 {
        return Err(ExecError::InvalidElf("PT_LOAD exceeds file size"));
    }
    let mem_end = ph
        .p_vaddr
        .checked_add(ph.p_memsz)
        .ok_or(ExecError::InvalidElf("PT_LOAD memory range overflow"))?;
    if mem_end >= USER_CANONICAL_LIMIT {
        return Err(ExecError::InvalidElf("PT_LOAD extends outside user space"));
    }
    Ok(())
}

fn map_load_segment(
    image: &[u8],
    ph: &Elf64ProgramHeader,
    pml4: PhysFrame,
) -> Result<(), ExecError> {
    let flags = load_flags(ph);
    let seg_start = align_down(ph.p_vaddr, PAGE_SIZE);
    let seg_end = align_up(
        ph.p_vaddr
            .checked_add(ph.p_memsz)
            .ok_or(ExecError::InvalidElf("segment end overflow"))?,
        PAGE_SIZE,
    );

    let mut page = seg_start;
    while page < seg_end {
        let frame = crate::vmm::alloc_zeroed_frame().ok_or(ExecError::OutOfMemory)?;
        copy_page_data(image, ph, page, frame);
        crate::vmm::map_page_in(pml4, VirtAddr::new(page), frame, flags)
            .map_err(ExecError::MapFailed)?;
        page += PAGE_SIZE;
    }

    Ok(())
}

fn copy_page_data(image: &[u8], ph: &Elf64ProgramHeader, page: u64, frame: PhysFrame) {
    if ph.p_filesz == 0 {
        return;
    }

    let page_end = page + PAGE_SIZE;
    let file_start_va = ph.p_vaddr;
    let file_end_va = ph.p_vaddr + ph.p_filesz;
    let copy_start = cmp::max(page, file_start_va);
    let copy_end = cmp::min(page_end, file_end_va);
    if copy_start >= copy_end {
        return;
    }

    let src_off = (ph.p_offset + (copy_start - file_start_va)) as usize;
    let dst_off = (copy_start - page) as usize;
    let len = (copy_end - copy_start) as usize;
    let dst = crate::vmm::phys_to_virt(frame.start_address()).as_mut_ptr::<u8>();

    unsafe {
        ptr::copy_nonoverlapping(image.as_ptr().add(src_off), dst.add(dst_off), len);
    }
}

fn load_flags(ph: &Elf64ProgramHeader) -> PageTableFlags {
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if ph.p_flags & PF_W != 0 {
        flags |= PageTableFlags::WRITABLE;
    }
    flags
}

fn read_struct<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(mem::size_of::<T>())?;
    if end > bytes.len() {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().add(offset) as *const T) })
}

fn write_u64(page: *mut u8, page_base: u64, addr: u64, value: u64) {
    let off = (addr - page_base) as usize;
    unsafe {
        ptr::write_unaligned(page.add(off) as *mut u64, value);
    }
}

fn write_byte(page: *mut u8, page_base: u64, addr: u64, value: u8) {
    let off = (addr - page_base) as usize;
    unsafe {
        ptr::write(page.add(off), value);
    }
}

fn write_bytes(page: *mut u8, page_base: u64, addr: u64, value: &[u8]) {
    let off = (addr - page_base) as usize;
    unsafe {
        ptr::copy_nonoverlapping(value.as_ptr(), page.add(off), value.len());
    }
}

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}
