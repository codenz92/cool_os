/// FAT32 filesystem access.
///
/// Supports 8.3 filenames only. Reads and writes sectors through `crate::ata`
/// and exposes a small mutation surface for creating empty files and folders.
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

const SECTOR_SIZE: usize = 512;
const DIR_ENTRY_SIZE: usize = 32;
const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_ARCHIVE: u8 = 0x20;
const ATTR_LFN: u8 = 0x0F;
const FAT_ENTRY_MASK: u32 = 0x0FFF_FFFF;
const FAT_FREE: u32 = 0x0000_0000;
const FAT_EOC: u32 = 0x0FFF_FFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    AlreadyExists,
    InvalidPath,
    Io,
    NoSpace,
    NotDirectory,
    NotFound,
    UnsupportedName,
}

impl FsError {
    pub const fn as_str(self) -> &'static str {
        match self {
            FsError::AlreadyExists => "already exists",
            FsError::InvalidPath => "invalid path",
            FsError::Io => "disk I/O failed",
            FsError::NoSpace => "filesystem full",
            FsError::NotDirectory => "not a directory",
            FsError::NotFound => "not found",
            FsError::UnsupportedName => "8.3 names only",
        }
    }
}

// ── BIOS Parameter Block (BPB) ────────────────────────────────────────────────

/// Parsed FAT32 BPB fields we need for navigation.
#[derive(Debug, Clone, Copy)]
pub struct Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub sectors_per_fat: u32,
    pub root_cluster: u32,
}

impl Bpb {
    pub fn load() -> Option<Self> {
        let mut sec = [0u8; SECTOR_SIZE];
        if !crate::ata::read_sector(0, &mut sec) {
            crate::println!("[fat32] BPB read_sector failed");
            return None;
        }

        if sec[510] != 0x55 || sec[511] != 0xAA {
            crate::println!(
                "[fat32] bad BPB sig: {:02x} {:02x}; first16={:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                sec[510], sec[511],
                sec[0], sec[1], sec[2], sec[3], sec[4], sec[5], sec[6], sec[7],
                sec[8], sec[9], sec[10], sec[11], sec[12], sec[13], sec[14], sec[15],
            );
            return None;
        }

        let bytes_per_sector = u16::from_le_bytes([sec[11], sec[12]]);
        let sectors_per_cluster = sec[13];
        let reserved_sectors = u16::from_le_bytes([sec[14], sec[15]]);
        let num_fats = sec[16];
        let fat16_spf = u16::from_le_bytes([sec[22], sec[23]]);
        let sectors_per_fat = if fat16_spf != 0 {
            fat16_spf as u32
        } else {
            u32::from_le_bytes([sec[36], sec[37], sec[38], sec[39]])
        };
        let root_cluster = u32::from_le_bytes([sec[44], sec[45], sec[46], sec[47]]);

        Some(Bpb {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_sectors,
            num_fats,
            sectors_per_fat,
            root_cluster,
        })
    }

    pub fn fat_start_lba(&self) -> u32 {
        self.reserved_sectors as u32
    }

    pub fn data_start_lba(&self) -> u32 {
        self.fat_start_lba() + self.num_fats as u32 * self.sectors_per_fat
    }

    pub fn cluster_lba(&self, cluster: u32) -> u32 {
        self.data_start_lba() + (cluster - 2) * self.sectors_per_cluster as u32
    }

    pub fn fat_entry_count(&self) -> u32 {
        self.sectors_per_fat * self.bytes_per_sector as u32 / 4
    }

    pub fn fat_next(&self, cluster: u32) -> Option<u32> {
        let byte_offset = cluster * 4;
        let sector_index = byte_offset / self.bytes_per_sector as u32;
        let byte_in_sec = (byte_offset % self.bytes_per_sector as u32) as usize;

        let lba = self.fat_start_lba() + sector_index;
        let mut sec = [0u8; SECTOR_SIZE];
        if !crate::ata::read_sector(lba, &mut sec) {
            return None;
        }
        let entry = u32::from_le_bytes([
            sec[byte_in_sec],
            sec[byte_in_sec + 1],
            sec[byte_in_sec + 2],
            sec[byte_in_sec + 3],
        ]) & FAT_ENTRY_MASK;
        Some(entry)
    }

    pub fn cluster_chain_clusters(&self, start: u32) -> Vec<u32> {
        let mut clusters = Vec::new();
        let mut cluster = start;
        loop {
            if cluster < 2 || cluster >= 0x0FFF_FFF8 {
                break;
            }
            clusters.push(cluster);
            match self.fat_next(cluster) {
                Some(next) if next >= 0x0FFF_FFF8 => break,
                Some(next) if next < 2 => break,
                Some(next) => cluster = next,
                None => break,
            }
        }
        clusters
    }

    pub fn cluster_chain_sectors(&self, start: u32) -> Vec<u32> {
        let mut sectors = Vec::new();
        for cluster in self.cluster_chain_clusters(start) {
            let lba = self.cluster_lba(cluster);
            for i in 0..self.sectors_per_cluster as u32 {
                sectors.push(lba + i);
            }
        }
        sectors
    }
}

// ── Directory entry ───────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct DirEntry {
    name: [u8; 11],
    attr: u8,
    first_cluster_hi: u16,
    first_cluster_lo: u16,
    file_size: u32,
}

impl DirEntry {
    fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < DIR_ENTRY_SIZE {
            return None;
        }
        if b[0] == 0x00 || b[0] == 0xE5 {
            return None;
        }
        let attr = b[11];
        if attr == ATTR_LFN {
            return None;
        }
        let mut name = [0u8; 11];
        name.copy_from_slice(&b[0..11]);
        Some(DirEntry {
            name,
            attr,
            first_cluster_hi: u16::from_le_bytes([b[20], b[21]]),
            first_cluster_lo: u16::from_le_bytes([b[26], b[27]]),
            file_size: u32::from_le_bytes([b[28], b[29], b[30], b[31]]),
        })
    }

    fn first_cluster(&self) -> u32 {
        (self.first_cluster_hi as u32) << 16 | self.first_cluster_lo as u32
    }

    fn is_dir(&self) -> bool {
        self.attr & ATTR_DIRECTORY != 0
    }

    fn name_as_string(&self) -> String {
        let mut base_buf = [0u8; 8];
        let mut base_len = 0usize;
        for &b in &self.name[..8] {
            if b == b' ' {
                break;
            }
            base_buf[base_len] = b;
            base_len += 1;
        }
        let mut ext_buf = [0u8; 3];
        let mut ext_len = 0usize;
        for &b in &self.name[8..] {
            if b == b' ' {
                break;
            }
            ext_buf[ext_len] = b;
            ext_len += 1;
        }
        let base_str = core::str::from_utf8(&base_buf[..base_len]).unwrap_or("????????");
        let ext_str = core::str::from_utf8(&ext_buf[..ext_len]).unwrap_or("");
        if ext_str.is_empty() {
            String::from(base_str)
        } else {
            let mut s = String::from(base_str);
            s.push('.');
            s.push_str(ext_str);
            s
        }
    }

    fn matches(&self, component: &[u8]) -> bool {
        self.name == name_to_83(component)
    }
}

#[derive(Clone, Copy)]
struct DirEntryLocation {
    entry: DirEntry,
    lba: u32,
    offset: usize,
}

fn name_to_83(s: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let s = if s.last() == Some(&0) {
        &s[..s.len() - 1]
    } else {
        s
    };
    let dot = s.iter().rposition(|&b| b == b'.');
    let (base, ext) = match dot {
        Some(p) => (&s[..p], &s[p + 1..]),
        None => (s, &b""[..]),
    };
    for (i, &b) in base.iter().take(8).enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    for (i, &b) in ext.iter().take(3).enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    out
}

fn name_to_83_checked(name: &str) -> Result<[u8; 11], FsError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }

    let bytes = name.as_bytes();
    let dot = bytes.iter().position(|&b| b == b'.');
    if let Some(first_dot) = dot {
        if bytes[first_dot + 1..].contains(&b'.') {
            return Err(FsError::UnsupportedName);
        }
    }

    let (base, ext) = match dot {
        Some(pos) => (&name[..pos], &name[pos + 1..]),
        None => (name, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return Err(FsError::UnsupportedName);
    }
    if !base.bytes().all(is_valid_short_name_char) || !ext.bytes().all(is_valid_short_name_char) {
        return Err(FsError::UnsupportedName);
    }

    Ok(name_to_83(name.as_bytes()))
}

fn is_valid_short_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'$' | b'~')
}

// ── Public API ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
}

pub fn list_dir(path: &str) -> Option<Vec<DirEntryInfo>> {
    let bpb = Bpb::load()?;
    let cluster = resolve_dir_cluster(&bpb, path).ok()?;

    let sectors = bpb.cluster_chain_sectors(cluster);
    let mut buf = [0u8; SECTOR_SIZE];
    let mut entries = Vec::new();
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return None;
        }
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            if let Some(entry) = DirEntry::from_bytes(&buf[offset..]) {
                let name = entry.name_as_string();
                if name == "." || name == ".." {
                    continue;
                }
                entries.push(DirEntryInfo {
                    name,
                    is_dir: entry.is_dir(),
                    size: entry.file_size,
                });
            } else if buf[offset] == 0x00 {
                return Some(entries);
            }
        }
    }
    Some(entries)
}

pub fn read_file(path: &str) -> Option<Vec<u8>> {
    let bpb = Bpb::load()?;
    let path = trim_abs_path(path).ok()?;
    let components: Vec<&[u8]> = path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();

    if components.is_empty() {
        return None;
    }

    let mut cluster = bpb.root_cluster;
    let last_idx = components.len() - 1;
    for (i, &component) in components.iter().enumerate() {
        let is_last = i == last_idx;
        match find_in_dir(&bpb, cluster, component)? {
            (_next_cluster, false) if is_last => {
                let entry = find_entry(&bpb, cluster, component)?;
                return read_clusters(&bpb, entry.first_cluster(), entry.file_size);
            }
            (next_cluster, true) if !is_last => cluster = next_cluster,
            (_next_cluster, false) if !is_last => return None,
            _ => return None,
        }
    }
    None
}

pub fn create_file(path: &str) -> Result<(), FsError> {
    let (bpb, parent_cluster, name83) = prepare_create(path)?;
    let entry = encode_dir_entry(name83, ATTR_ARCHIVE, 0, 0);
    append_dir_entry(&bpb, parent_cluster, &entry)
}

pub fn create_dir(path: &str) -> Result<(), FsError> {
    let (bpb, parent_cluster, name83) = prepare_create(path)?;
    let new_cluster = alloc_cluster(&bpb)?;

    if let Err(err) = init_directory_cluster(&bpb, new_cluster, parent_cluster) {
        let _ = free_single_cluster(&bpb, new_cluster);
        return Err(err);
    }

    let entry = encode_dir_entry(name83, ATTR_DIRECTORY, new_cluster, 0);
    if let Err(err) = append_dir_entry(&bpb, parent_cluster, &entry) {
        let _ = free_single_cluster(&bpb, new_cluster);
        return Err(err);
    }

    Ok(())
}

pub fn rename(path: &str, new_name: &str) -> Result<(), FsError> {
    let (parent_path, old_name) = split_parent_and_name(path)?;
    if old_name.eq_ignore_ascii_case(new_name) {
        return Ok(());
    }
    let new_name83 = name_to_83_checked(new_name)?;
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    if find_entry(&bpb, parent_cluster, new_name.as_bytes()).is_some() {
        return Err(FsError::AlreadyExists);
    }
    let location =
        find_entry_location(&bpb, parent_cluster, old_name.as_bytes()).ok_or(FsError::NotFound)?;
    let mut sec = [0u8; SECTOR_SIZE];
    read_sector_exact(location.lba, &mut sec)?;
    sec[location.offset..location.offset + 11].copy_from_slice(&new_name83);
    write_sector_exact(location.lba, &sec)
}

pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    let location =
        find_entry_location(&bpb, parent_cluster, name.as_bytes()).ok_or(FsError::NotFound)?;
    if location.entry.is_dir() {
        return Err(FsError::InvalidPath);
    }

    let old_first_cluster = location.entry.first_cluster();
    let mut new_first_cluster = 0u32;
    let mut new_chain = Vec::new();
    if !data.is_empty() {
        let sectors_needed = (data.len() + SECTOR_SIZE - 1) / SECTOR_SIZE;
        let clusters_needed = (sectors_needed + bpb.sectors_per_cluster as usize - 1)
            / bpb.sectors_per_cluster as usize;
        new_chain = alloc_cluster_chain(&bpb, clusters_needed)?;
        new_first_cluster = new_chain[0];
        if let Err(err) = write_data_to_clusters(&bpb, &new_chain, data) {
            free_cluster_list(&bpb, &new_chain)?;
            return Err(err);
        }
    }

    let mut sec = [0u8; SECTOR_SIZE];
    read_sector_exact(location.lba, &mut sec)?;
    sec[location.offset + 20..location.offset + 22]
        .copy_from_slice(&((new_first_cluster >> 16) as u16).to_le_bytes());
    sec[location.offset + 26..location.offset + 28]
        .copy_from_slice(&(new_first_cluster as u16).to_le_bytes());
    sec[location.offset + 28..location.offset + 32]
        .copy_from_slice(&(data.len() as u32).to_le_bytes());
    if let Err(err) = write_sector_exact(location.lba, &sec) {
        if !new_chain.is_empty() {
            let _ = free_cluster_list(&bpb, &new_chain);
        }
        return Err(err);
    }

    if old_first_cluster >= 2 {
        free_cluster_chain(&bpb, old_first_cluster)?;
    }

    Ok(())
}

// ── Path + lookup helpers ─────────────────────────────────────────────────────

fn trim_abs_path(path: &str) -> Result<&str, FsError> {
    if !path.starts_with('/') {
        return Err(FsError::InvalidPath);
    }
    let mut end = path.len();
    while end > 1 && path.as_bytes()[end - 1] == b'/' {
        end -= 1;
    }
    Ok(&path[..end])
}

fn split_parent_and_name(path: &str) -> Result<(String, String), FsError> {
    let path = trim_abs_path(path)?;
    if path == "/" {
        return Err(FsError::InvalidPath);
    }
    let slash = path.rfind('/').ok_or(FsError::InvalidPath)?;
    let parent = if slash == 0 {
        String::from("/")
    } else {
        String::from(&path[..slash])
    };
    let name = String::from(&path[slash + 1..]);
    if name.is_empty() {
        return Err(FsError::InvalidPath);
    }
    Ok((parent, name))
}

fn prepare_create(path: &str) -> Result<(Bpb, u32, [u8; 11]), FsError> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let name83 = name_to_83_checked(&name)?;
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    if find_entry(&bpb, parent_cluster, name.as_bytes()).is_some() {
        return Err(FsError::AlreadyExists);
    }
    Ok((bpb, parent_cluster, name83))
}

fn resolve_dir_cluster(bpb: &Bpb, path: &str) -> Result<u32, FsError> {
    let path = trim_abs_path(path)?;
    let mut cluster = bpb.root_cluster;
    let components: Vec<&[u8]> = path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();

    for component in components {
        let entry = find_entry(bpb, cluster, component).ok_or(FsError::NotFound)?;
        if !entry.is_dir() {
            return Err(FsError::NotDirectory);
        }
        cluster = entry.first_cluster();
    }

    Ok(cluster)
}

fn find_in_dir(bpb: &Bpb, dir_cluster: u32, name: &[u8]) -> Option<(u32, bool)> {
    let entry = find_entry(bpb, dir_cluster, name)?;
    Some((entry.first_cluster(), entry.is_dir()))
}

fn find_entry(bpb: &Bpb, dir_cluster: u32, name: &[u8]) -> Option<DirEntry> {
    find_entry_location(bpb, dir_cluster, name).map(|location| location.entry)
}

fn find_entry_location(bpb: &Bpb, dir_cluster: u32, name: &[u8]) -> Option<DirEntryLocation> {
    let sectors = bpb.cluster_chain_sectors(dir_cluster);
    let mut buf = [0u8; SECTOR_SIZE];
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return None;
        }
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            if let Some(entry) = DirEntry::from_bytes(&buf[offset..]) {
                if entry.matches(name) {
                    return Some(DirEntryLocation { entry, lba, offset });
                }
            } else if buf[offset] == 0x00 {
                return None;
            }
        }
    }
    None
}

fn read_clusters(bpb: &Bpb, start: u32, size: u32) -> Option<Vec<u8>> {
    let sectors = bpb.cluster_chain_sectors(start);
    let mut data = Vec::with_capacity(size as usize);
    let mut buf = [0u8; SECTOR_SIZE];
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return None;
        }
        data.extend_from_slice(&buf);
    }
    data.truncate(size as usize);
    Some(data)
}

// ── Mutation helpers ──────────────────────────────────────────────────────────

fn alloc_cluster(bpb: &Bpb) -> Result<u32, FsError> {
    let entries_per_sector = SECTOR_SIZE / 4;
    let total_entries = bpb.fat_entry_count();
    let mut sec = [0u8; SECTOR_SIZE];

    for sector_index in 0..bpb.sectors_per_fat {
        read_sector_exact(bpb.fat_start_lba() + sector_index, &mut sec)?;
        for entry_idx in 0..entries_per_sector {
            let cluster = sector_index * entries_per_sector as u32 + entry_idx as u32;
            if cluster < 2 || cluster >= total_entries {
                continue;
            }
            let byte = entry_idx * 4;
            let entry =
                u32::from_le_bytes([sec[byte], sec[byte + 1], sec[byte + 2], sec[byte + 3]])
                    & FAT_ENTRY_MASK;
            if entry == FAT_FREE {
                fat_write_entry(bpb, cluster, FAT_EOC)?;
                zero_cluster(bpb, cluster)?;
                return Ok(cluster);
            }
        }
    }

    Err(FsError::NoSpace)
}

fn free_single_cluster(bpb: &Bpb, cluster: u32) -> Result<(), FsError> {
    fat_write_entry(bpb, cluster, FAT_FREE)
}

fn alloc_cluster_chain(bpb: &Bpb, count: usize) -> Result<Vec<u32>, FsError> {
    let mut chain = Vec::new();
    for _ in 0..count {
        match alloc_cluster(bpb) {
            Ok(cluster) => chain.push(cluster),
            Err(err) => {
                free_cluster_list(bpb, &chain)?;
                return Err(err);
            }
        }
    }
    for pair in chain.windows(2) {
        fat_write_entry(bpb, pair[0], pair[1])?;
    }
    Ok(chain)
}

fn write_data_to_clusters(bpb: &Bpb, chain: &[u32], data: &[u8]) -> Result<(), FsError> {
    let cluster_bytes = bpb.sectors_per_cluster as usize * SECTOR_SIZE;
    let mut sector = [0u8; SECTOR_SIZE];
    for (cluster_idx, &cluster) in chain.iter().enumerate() {
        let cluster_start = cluster_idx * cluster_bytes;
        let base_lba = bpb.cluster_lba(cluster);
        for sector_idx in 0..bpb.sectors_per_cluster as usize {
            sector.fill(0);
            let chunk_start = cluster_start + sector_idx * SECTOR_SIZE;
            if chunk_start < data.len() {
                let chunk_end = (chunk_start + SECTOR_SIZE).min(data.len());
                sector[..chunk_end - chunk_start].copy_from_slice(&data[chunk_start..chunk_end]);
            }
            write_sector_exact(base_lba + sector_idx as u32, &sector)?;
        }
    }
    Ok(())
}

fn free_cluster_list(bpb: &Bpb, clusters: &[u32]) -> Result<(), FsError> {
    for &cluster in clusters {
        fat_write_entry(bpb, cluster, FAT_FREE)?;
    }
    Ok(())
}

fn free_cluster_chain(bpb: &Bpb, start: u32) -> Result<(), FsError> {
    let chain = bpb.cluster_chain_clusters(start);
    free_cluster_list(bpb, &chain)
}

fn fat_write_entry(bpb: &Bpb, cluster: u32, value: u32) -> Result<(), FsError> {
    let byte_offset = cluster * 4;
    let sector_index = byte_offset / bpb.bytes_per_sector as u32;
    let byte_in_sec = (byte_offset % bpb.bytes_per_sector as u32) as usize;

    for fat_copy in 0..bpb.num_fats as u32 {
        let lba = bpb.fat_start_lba() + fat_copy * bpb.sectors_per_fat + sector_index;
        let mut sec = [0u8; SECTOR_SIZE];
        read_sector_exact(lba, &mut sec)?;
        let current = u32::from_le_bytes([
            sec[byte_in_sec],
            sec[byte_in_sec + 1],
            sec[byte_in_sec + 2],
            sec[byte_in_sec + 3],
        ]);
        let next = (current & !FAT_ENTRY_MASK) | (value & FAT_ENTRY_MASK);
        sec[byte_in_sec..byte_in_sec + 4].copy_from_slice(&next.to_le_bytes());
        write_sector_exact(lba, &sec)?;
    }

    Ok(())
}

fn zero_cluster(bpb: &Bpb, cluster: u32) -> Result<(), FsError> {
    let zero = [0u8; SECTOR_SIZE];
    let lba = bpb.cluster_lba(cluster);
    for i in 0..bpb.sectors_per_cluster as u32 {
        write_sector_exact(lba + i, &zero)?;
    }
    Ok(())
}

fn init_directory_cluster(bpb: &Bpb, cluster: u32, parent_cluster: u32) -> Result<(), FsError> {
    let mut first_sector = [0u8; SECTOR_SIZE];
    first_sector[..DIR_ENTRY_SIZE].copy_from_slice(&encode_dir_entry(
        dot_name(),
        ATTR_DIRECTORY,
        cluster,
        0,
    ));
    first_sector[DIR_ENTRY_SIZE..DIR_ENTRY_SIZE * 2].copy_from_slice(&encode_dir_entry(
        dotdot_name(),
        ATTR_DIRECTORY,
        parent_cluster,
        0,
    ));
    write_sector_exact(bpb.cluster_lba(cluster), &first_sector)
}

fn append_dir_entry(
    bpb: &Bpb,
    dir_cluster: u32,
    entry: &[u8; DIR_ENTRY_SIZE],
) -> Result<(), FsError> {
    let clusters = bpb.cluster_chain_clusters(dir_cluster);
    let mut sec = [0u8; SECTOR_SIZE];

    for &cluster in &clusters {
        let base_lba = bpb.cluster_lba(cluster);
        for sector_idx in 0..bpb.sectors_per_cluster as u32 {
            let lba = base_lba + sector_idx;
            read_sector_exact(lba, &mut sec)?;
            for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
                let first = sec[offset];
                if first == 0xE5 || first == 0x00 {
                    sec[offset..offset + DIR_ENTRY_SIZE].copy_from_slice(entry);
                    if first == 0x00 && offset + DIR_ENTRY_SIZE < SECTOR_SIZE {
                        sec[offset + DIR_ENTRY_SIZE] = 0x00;
                    }
                    write_sector_exact(lba, &sec)?;
                    return Ok(());
                }
            }
        }
    }

    let last_cluster = *clusters.last().ok_or(FsError::Io)?;
    let new_cluster = alloc_cluster(bpb)?;
    let mut first_sector = [0u8; SECTOR_SIZE];
    first_sector[..DIR_ENTRY_SIZE].copy_from_slice(entry);
    if let Err(err) = write_sector_exact(bpb.cluster_lba(new_cluster), &first_sector) {
        let _ = free_single_cluster(bpb, new_cluster);
        return Err(err);
    }
    if let Err(err) = fat_write_entry(bpb, last_cluster, new_cluster) {
        let _ = free_single_cluster(bpb, new_cluster);
        return Err(err);
    }
    Ok(())
}

fn encode_dir_entry(
    name: [u8; 11],
    attr: u8,
    first_cluster: u32,
    file_size: u32,
) -> [u8; DIR_ENTRY_SIZE] {
    let mut entry = [0u8; DIR_ENTRY_SIZE];
    entry[0..11].copy_from_slice(&name);
    entry[11] = attr;
    entry[20..22].copy_from_slice(&((first_cluster >> 16) as u16).to_le_bytes());
    entry[26..28].copy_from_slice(&(first_cluster as u16).to_le_bytes());
    entry[28..32].copy_from_slice(&file_size.to_le_bytes());
    entry
}

fn dot_name() -> [u8; 11] {
    let mut name = [b' '; 11];
    name[0] = b'.';
    name
}

fn dotdot_name() -> [u8; 11] {
    let mut name = [b' '; 11];
    name[0] = b'.';
    name[1] = b'.';
    name
}

fn read_sector_exact(lba: u32, buf: &mut [u8; SECTOR_SIZE]) -> Result<(), FsError> {
    if crate::ata::read_sector(lba, buf) {
        Ok(())
    } else {
        Err(FsError::Io)
    }
}

fn write_sector_exact(lba: u32, buf: &[u8; SECTOR_SIZE]) -> Result<(), FsError> {
    if crate::ata::write_sector(lba, buf) {
        Ok(())
    } else {
        Err(FsError::Io)
    }
}
