/// Virtual Filesystem layer (Phase 11/13).
///
/// File descriptors are now task-local. Each task owns a small fd table that
/// points at shared open objects, so inherited or duplicated descriptors refer
/// to the same underlying file/pipe state instead of a single global fd slot.
extern crate alloc;
use alloc::{format, string::String, vec::Vec};
use core::arch::asm;
use spin::Mutex;
use x86_64::structures::paging::PhysFrame;

const MAX_FDS: usize = 16;
const PIPE_SIZE: usize = 512;

struct OpenFile {
    data: Vec<u8>,
    offset: usize,
}

struct Pipe {
    buf: [u8; PIPE_SIZE],
    head: usize,
    tail: usize,
    len: usize,
    readers: usize,
    writers: usize,
    waiting_reader: Option<usize>,
}

impl Pipe {
    fn new() -> Self {
        Self {
            buf: [0; PIPE_SIZE],
            head: 0,
            tail: 0,
            len: 0,
            readers: 0,
            writers: 0,
            waiting_reader: None,
        }
    }

    fn read(&mut self, out: &mut [u8], len: usize) -> usize {
        let mut n = 0usize;
        let want = len.min(out.len());
        while n < want && self.len > 0 {
            out[n] = self.buf[self.tail];
            self.tail = (self.tail + 1) % PIPE_SIZE;
            self.len -= 1;
            n += 1;
        }
        n
    }

    fn write(&mut self, input: &[u8]) -> usize {
        let mut n = 0usize;
        while n < input.len() && self.len < PIPE_SIZE {
            self.buf[self.head] = input[n];
            self.head = (self.head + 1) % PIPE_SIZE;
            self.len += 1;
            n += 1;
        }
        n
    }
}

enum PipeReadResult {
    Data(usize),
    WouldBlock,
    Eof,
}

struct SharedMemRegion {
    frames: Vec<PhysFrame>,
    refcount: usize,
}

impl SharedMemRegion {
    fn new(frames: Vec<PhysFrame>) -> Self {
        Self {
            frames,
            refcount: 1,
        }
    }
}

static SHMEM_REGIONS: Mutex<Vec<Option<SharedMemRegion>>> = Mutex::new(Vec::new());
static SHMEM_NEXT_ID: Mutex<usize> = Mutex::new(1);

enum SharedKind {
    File(OpenFile),
    Pipe(Pipe),
}

struct SharedObject {
    refs: usize,
    kind: SharedKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FdAccess {
    File,
    PipeRead,
    PipeWrite,
}

#[derive(Clone, Copy)]
struct LocalFd {
    object: usize,
    access: FdAccess,
}

#[derive(Clone, Copy)]
struct TaskFdTable {
    entries: [Option<LocalFd>; MAX_FDS],
}

impl TaskFdTable {
    const fn new() -> Self {
        Self {
            entries: [None; MAX_FDS],
        }
    }

    fn alloc_fd(&mut self, entry: LocalFd) -> Option<usize> {
        for fd in 3..MAX_FDS {
            if self.entries[fd].is_none() {
                self.entries[fd] = Some(entry);
                return Some(fd);
            }
        }
        None
    }

    fn install_fd(&mut self, fd: usize, entry: LocalFd) -> bool {
        if fd < 3 || fd >= MAX_FDS || self.entries[fd].is_some() {
            return false;
        }
        self.entries[fd] = Some(entry);
        true
    }
}

struct VfsState {
    objects: Vec<Option<SharedObject>>,
    task_fds: Vec<TaskFdTable>,
}

impl VfsState {
    const fn new() -> Self {
        Self {
            objects: Vec::new(),
            task_fds: Vec::new(),
        }
    }

    fn ensure_task(&mut self, task_id: usize) {
        while self.task_fds.len() <= task_id {
            self.task_fds.push(TaskFdTable::new());
        }
    }

    fn alloc_object(&mut self, kind: SharedKind) -> usize {
        for (id, slot) in self.objects.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(SharedObject { refs: 0, kind });
                return id;
            }
        }

        let id = self.objects.len();
        self.objects.push(Some(SharedObject { refs: 0, kind }));
        id
    }

    fn current_entry(&self, task_id: usize, fd: usize) -> Option<LocalFd> {
        self.task_fds
            .get(task_id)
            .and_then(|table| table.entries.get(fd))
            .copied()
            .flatten()
    }

    fn retain_fd(&mut self, entry: LocalFd) -> bool {
        let Some(object) = self.objects.get_mut(entry.object).and_then(Option::as_mut) else {
            return false;
        };

        object.refs += 1;
        if let SharedKind::Pipe(pipe) = &mut object.kind {
            match entry.access {
                FdAccess::PipeRead => pipe.readers += 1,
                FdAccess::PipeWrite => pipe.writers += 1,
                FdAccess::File => {}
            }
        }
        true
    }

    fn release_fd(&mut self, entry: LocalFd) -> Option<usize> {
        let mut wake_task = None;

        let Some(object) = self.objects.get_mut(entry.object).and_then(Option::as_mut) else {
            return None;
        };

        object.refs = object.refs.saturating_sub(1);
        if let SharedKind::Pipe(pipe) = &mut object.kind {
            match entry.access {
                FdAccess::PipeRead => {
                    pipe.readers = pipe.readers.saturating_sub(1);
                    if pipe.readers == 0 {
                        pipe.waiting_reader = None;
                    }
                }
                FdAccess::PipeWrite => {
                    pipe.writers = pipe.writers.saturating_sub(1);
                    if pipe.writers == 0 {
                        wake_task = pipe.waiting_reader.take();
                    }
                }
                FdAccess::File => {}
            }
        }

        let should_drop = self.objects[entry.object]
            .as_ref()
            .map(|object| object.refs == 0)
            .unwrap_or(false);
        if should_drop {
            self.objects[entry.object] = None;
        }

        wake_task
    }
}

static VFS: Mutex<VfsState> = Mutex::new(VfsState::new());

pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    if parts.is_empty() {
        return String::from("/");
    }
    let mut out = String::new();
    for part in parts {
        out.push('/');
        out.push_str(part);
    }
    out
}

pub fn mount_lines() -> Vec<String> {
    alloc::vec![
        String::from("/ type=fat32 flags=rw,relatime,normalized-paths"),
        String::from("/DEV type=devfs flags=ro,generated"),
        String::from("/APPS type=appfs flags=rw,manifest-validated"),
        format!("fd tables={} max_fd={}", VFS.lock().task_fds.len(), MAX_FDS),
    ]
}

pub fn init_task(task_id: usize) {
    let mut vfs = VFS.lock();
    vfs.ensure_task(task_id);
    vfs.task_fds[task_id] = TaskFdTable::new();
}

pub fn drop_task(task_id: usize) {
    let wake_tasks = {
        let mut vfs = VFS.lock();
        if task_id >= vfs.task_fds.len() {
            return;
        }

        let mut wake_tasks = Vec::new();
        for fd in 3..MAX_FDS {
            let entry = vfs.task_fds[task_id].entries[fd].take();
            if let Some(entry) = entry {
                if let Some(task) = vfs.release_fd(entry) {
                    wake_tasks.push(task);
                }
            }
        }
        wake_tasks
    };

    for task_id in wake_tasks {
        crate::scheduler::unblock(task_id);
    }
}

pub fn inherit_fds(parent_task: usize, child_task: usize, mappings: &[(usize, usize)]) -> bool {
    let mut vfs = VFS.lock();
    vfs.ensure_task(parent_task);
    vfs.ensure_task(child_task);

    let mut installed: Vec<usize> = Vec::new();
    for &(parent_fd, child_fd) in mappings {
        let Some(entry) = vfs.current_entry(parent_task, parent_fd) else {
            for fd in installed {
                if let Some(rollback) = vfs.task_fds[child_task].entries[fd].take() {
                    vfs.release_fd(rollback);
                }
            }
            return false;
        };

        if !vfs.retain_fd(entry) || !vfs.task_fds[child_task].install_fd(child_fd, entry) {
            if vfs.current_entry(child_task, child_fd).is_none() {
                vfs.release_fd(entry);
            }
            for fd in installed {
                if let Some(rollback) = vfs.task_fds[child_task].entries[fd].take() {
                    vfs.release_fd(rollback);
                }
            }
            return false;
        }

        installed.push(child_fd);
    }

    true
}

pub fn vfs_open(path: &str) -> usize {
    let path = normalize_path(path);
    if !crate::security::can_read_path(&path) {
        return usize::MAX;
    }
    let data = match crate::fat32::read_file(&path) {
        Some(data) => data,
        None => return usize::MAX,
    };

    let task_id = crate::scheduler::current_task_id();
    let mut vfs = VFS.lock();
    vfs.ensure_task(task_id);

    let object = vfs.alloc_object(SharedKind::File(OpenFile { data, offset: 0 }));
    let entry = LocalFd {
        object,
        access: FdAccess::File,
    };
    if !vfs.retain_fd(entry) {
        return usize::MAX;
    }

    match vfs.task_fds[task_id].alloc_fd(entry) {
        Some(fd) => fd,
        None => {
            vfs.release_fd(entry);
            usize::MAX
        }
    }
}

pub fn vfs_pipe() -> Option<(usize, usize)> {
    let task_id = crate::scheduler::current_task_id();
    let mut vfs = VFS.lock();
    vfs.ensure_task(task_id);

    let object = vfs.alloc_object(SharedKind::Pipe(Pipe::new()));
    let read_entry = LocalFd {
        object,
        access: FdAccess::PipeRead,
    };
    let write_entry = LocalFd {
        object,
        access: FdAccess::PipeWrite,
    };

    if !vfs.retain_fd(read_entry) {
        return None;
    }
    let read_fd = match vfs.task_fds[task_id].alloc_fd(read_entry) {
        Some(fd) => fd,
        None => {
            vfs.release_fd(read_entry);
            return None;
        }
    };

    if !vfs.retain_fd(write_entry) {
        vfs.task_fds[task_id].entries[read_fd] = None;
        vfs.release_fd(read_entry);
        return None;
    }
    let write_fd = match vfs.task_fds[task_id].alloc_fd(write_entry) {
        Some(fd) => fd,
        None => {
            vfs.task_fds[task_id].entries[read_fd] = None;
            vfs.release_fd(read_entry);
            vfs.release_fd(write_entry);
            return None;
        }
    };

    Some((read_fd, write_fd))
}

#[allow(dead_code)]
pub fn vfs_read(fd: usize, buf: &mut [u8], len: usize) -> usize {
    let task_id = crate::scheduler::current_task_id();
    let mut vfs = VFS.lock();
    let Some(entry) = vfs.current_entry(task_id, fd) else {
        return usize::MAX;
    };

    let Some(object) = vfs.objects.get_mut(entry.object).and_then(Option::as_mut) else {
        return usize::MAX;
    };

    match (&mut object.kind, entry.access) {
        (SharedKind::File(file), FdAccess::File) => {
            let available = file.data.len().saturating_sub(file.offset);
            let to_read = len.min(available).min(buf.len());
            buf[..to_read].copy_from_slice(&file.data[file.offset..file.offset + to_read]);
            file.offset += to_read;
            to_read
        }
        (SharedKind::Pipe(pipe), FdAccess::PipeRead) => match pipe_read(pipe, buf, len) {
            PipeReadResult::Data(n) => n,
            PipeReadResult::Eof | PipeReadResult::WouldBlock => 0,
        },
        _ => usize::MAX,
    }
}

pub fn vfs_write(fd: usize, buf: &[u8]) -> usize {
    let task_id = crate::scheduler::current_task_id();
    let (n, wake_task) = {
        let mut vfs = VFS.lock();
        let Some(entry) = vfs.current_entry(task_id, fd) else {
            return usize::MAX;
        };

        let Some(object) = vfs.objects.get_mut(entry.object).and_then(Option::as_mut) else {
            return usize::MAX;
        };

        match (&mut object.kind, entry.access) {
            (SharedKind::Pipe(pipe), FdAccess::PipeWrite) => {
                if pipe.readers == 0 {
                    return usize::MAX;
                }

                let n = pipe.write(buf);
                let wake_task = if n > 0 {
                    pipe.waiting_reader.take()
                } else {
                    None
                };
                (n, wake_task)
            }
            _ => return usize::MAX,
        }
    };

    if let Some(task_id) = wake_task {
        crate::scheduler::unblock(task_id);
    }
    n
}

pub fn vfs_read_blocking(fd: usize, buf: &mut [u8], len: usize) -> usize {
    loop {
        let result = {
            let task_id = crate::scheduler::current_task_id();
            let mut vfs = VFS.lock();
            let Some(entry) = vfs.current_entry(task_id, fd) else {
                return usize::MAX;
            };

            let Some(object) = vfs.objects.get_mut(entry.object).and_then(Option::as_mut) else {
                return usize::MAX;
            };

            match (&mut object.kind, entry.access) {
                (SharedKind::File(file), FdAccess::File) => {
                    let available = file.data.len().saturating_sub(file.offset);
                    let to_read = len.min(available).min(buf.len());
                    buf[..to_read].copy_from_slice(&file.data[file.offset..file.offset + to_read]);
                    file.offset += to_read;
                    return to_read;
                }
                (SharedKind::Pipe(pipe), FdAccess::PipeRead) => {
                    let result = pipe_read(pipe, buf, len);
                    if matches!(result, PipeReadResult::WouldBlock) {
                        pipe.waiting_reader = Some(task_id);
                    }
                    result
                }
                _ => return usize::MAX,
            }
        };

        match result {
            PipeReadResult::Data(n) => return n,
            PipeReadResult::Eof => return 0,
            PipeReadResult::WouldBlock => {
                crate::scheduler::block_current();
                while crate::scheduler::current_task_blocked() {
                    unsafe {
                        asm!("sti; hlt", options(nomem, nostack));
                    }
                }
                x86_64::instructions::interrupts::disable();
            }
        }
    }
}

pub fn vfs_close(fd: usize) {
    let task_id = crate::scheduler::current_task_id();
    let wake_task = {
        let mut vfs = VFS.lock();
        let Some(table) = vfs.task_fds.get_mut(task_id) else {
            return;
        };
        let Some(entry) = table.entries.get_mut(fd).and_then(Option::take) else {
            return;
        };
        vfs.release_fd(entry)
    };

    if let Some(task_id) = wake_task {
        crate::scheduler::unblock(task_id);
    }
}

pub fn vfs_dup(fd: usize) -> usize {
    let task_id = crate::scheduler::current_task_id();
    let mut vfs = VFS.lock();
    vfs.ensure_task(task_id);

    let Some(entry) = vfs.current_entry(task_id, fd) else {
        return usize::MAX;
    };
    if !vfs.retain_fd(entry) {
        return usize::MAX;
    }

    match vfs.task_fds[task_id].alloc_fd(entry) {
        Some(new_fd) => new_fd,
        None => {
            vfs.release_fd(entry);
            usize::MAX
        }
    }
}

fn pipe_read(pipe: &mut Pipe, buf: &mut [u8], len: usize) -> PipeReadResult {
    if pipe.len > 0 {
        PipeReadResult::Data(pipe.read(buf, len))
    } else if pipe.writers == 0 {
        PipeReadResult::Eof
    } else {
        PipeReadResult::WouldBlock
    }
}

pub fn vfs_shmem_create(len: usize) -> usize {
    let pages = (len + 4095) / 4096;
    let mut frames: Vec<PhysFrame> = Vec::new();
    for _ in 0..pages {
        let frame = match crate::vmm::alloc_zeroed_frame() {
            Some(f) => f,
            None => return usize::MAX,
        };
        frames.push(frame);
    }

    let id = {
        let mut next_id = SHMEM_NEXT_ID.lock();
        let id = *next_id;
        *next_id = id + 1;
        id
    };

    let mut regions = SHMEM_REGIONS.lock();
    while regions.len() <= id {
        regions.push(None);
    }
    regions[id] = Some(SharedMemRegion::new(frames));

    id
}

pub fn vfs_shmem_map(id: usize, pml4: PhysFrame) -> u64 {
    let addr = {
        let mut regions = SHMEM_REGIONS.lock();
        let Some(Some(region)) = regions.get_mut(id) else {
            return u64::MAX;
        };

        let already_mapped = region.refcount > 1;
        region.refcount += 1;

        let base_addr: u64 = 0x0000_4000_0000_0000;
        let offset = (id as u64 - 1) * (64 * 1024 * 1024);

        if already_mapped {
            return base_addr + offset;
        }

        let flags = x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE;

        let mut virt_addr = base_addr + offset;
        for frame in &region.frames {
            let virt = x86_64::VirtAddr::new(virt_addr);
            let _ = crate::vmm::map_page_in(pml4, virt, *frame, flags);
            virt_addr += 4096;
        }

        base_addr + offset
    };

    addr
}
