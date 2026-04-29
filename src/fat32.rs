/// FAT32 filesystem access.
///
/// Reads and writes sectors through `crate::ata`, supports short names plus
/// FAT long filename entries, and exposes create/write/rename/delete/copy
/// primitives used by the shell and file manager.
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
const LFN_CHAR_OFFSETS: [usize; 13] = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    AlreadyExists,
    InvalidPath,
    Io,
    NoSpace,
    NotDirectory,
    NotEmpty,
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
            FsError::NotEmpty => "directory not empty",
            FsError::NotFound => "not found",
            FsError::UnsupportedName => "invalid name",
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

struct DirEntryLocation {
    entry: DirEntry,
    lba: u32,
    offset: usize,
    lfn_locations: Vec<(u32, usize)>,
}

struct FatName {
    short: [u8; 11],
    lfn_entries: Vec<[u8; DIR_ENTRY_SIZE]>,
}

fn name_to_83(s: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let s = strip_trailing_nul(s);
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

// ── LFN helpers ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct LfnFragment {
    seq: u8,
    is_last: bool,
    checksum: u8,
    chars: [u16; 13],
}

fn lfn_checksum(name83: &[u8; 11]) -> u8 {
    name83
        .iter()
        .fold(0u8, |sum, &b| (sum >> 1 | (sum & 1) << 7).wrapping_add(b))
}

fn read_lfn_chars(b: &[u8]) -> [u16; 13] {
    let mut chars = [0u16; 13];
    for (i, &off) in LFN_CHAR_OFFSETS.iter().enumerate() {
        chars[i] = u16::from_le_bytes([b[off], b[off + 1]]);
    }
    chars
}

fn read_lfn_fragment(b: &[u8]) -> Option<LfnFragment> {
    if b.len() < DIR_ENTRY_SIZE || b[11] != ATTR_LFN {
        return None;
    }
    let seq = b[0] & 0x3F;
    if seq == 0 || b[12] != 0 || b[26] != 0 || b[27] != 0 {
        return None;
    }
    Some(LfnFragment {
        seq,
        is_last: b[0] & 0x40 != 0,
        checksum: b[13],
        chars: read_lfn_chars(b),
    })
}

fn assemble_lfn(fragments: &[(u8, [u16; 13])]) -> Option<String> {
    if fragments.is_empty() {
        return None;
    }
    let mut sorted = fragments.to_vec();
    sorted.sort_by_key(|(seq, _)| *seq);
    for (i, (seq, _)) in sorted.iter().enumerate() {
        if *seq as usize != i + 1 {
            return None;
        }
    }
    let mut utf16: Vec<u16> = Vec::new();
    for (_, chars) in &sorted {
        for &ch in chars.iter() {
            if ch == 0x0000 {
                return utf16_to_string(&utf16);
            }
            if ch != 0xFFFF {
                utf16.push(ch);
            }
        }
    }
    utf16_to_string(&utf16)
}

fn utf16_to_string(utf16: &[u16]) -> Option<String> {
    let mut s = String::new();
    let mut i = 0usize;
    while i < utf16.len() {
        let ch = utf16[i];
        if (0xD800..0xDC00).contains(&ch) {
            if i + 1 >= utf16.len() {
                return None;
            }
            let low = utf16[i + 1];
            if !(0xDC00..0xE000).contains(&low) {
                return None;
            }
            let code = 0x10000u32 + ((ch as u32 - 0xD800) << 10) + (low as u32 - 0xDC00);
            s.push(char::from_u32(code)?);
            i += 2;
        } else if (0xDC00..0xE000).contains(&ch) {
            return None;
        } else {
            s.push(char::from_u32(ch as u32)?);
            i += 1;
        }
    }
    Some(s)
}

fn assemble_lfn_for_entry(entry: &DirEntry, fragments: &[LfnFragment]) -> Option<String> {
    if fragments.is_empty() {
        return None;
    }

    let checksum = lfn_checksum(&entry.name);
    if fragments
        .iter()
        .any(|fragment| fragment.checksum != checksum)
    {
        return None;
    }

    let last = fragments.iter().find(|fragment| fragment.is_last)?;
    if fragments.iter().filter(|fragment| fragment.is_last).count() != 1 {
        return None;
    }
    if last.seq as usize != fragments.len() {
        return None;
    }

    let mut ordered = Vec::new();
    for fragment in fragments {
        ordered.push((fragment.seq, fragment.chars));
    }
    assemble_lfn(&ordered)
}

fn strip_trailing_nul(s: &[u8]) -> &[u8] {
    if s.last() == Some(&0) {
        &s[..s.len() - 1]
    } else {
        s
    }
}

fn lfn_name_matches(lfn_name: &str, component: &[u8]) -> bool {
    core::str::from_utf8(strip_trailing_nul(component))
        .map(|component| lfn_name.eq_ignore_ascii_case(component))
        .unwrap_or(false)
}

fn validate_lfn_name(name: &str) -> Result<(), FsError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(FsError::UnsupportedName);
    }
    let mut utf16_len = 0usize;
    for ch in name.chars() {
        if (ch as u32) < 0x20 || matches!(ch, '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|')
        {
            return Err(FsError::UnsupportedName);
        }
        utf16_len += ch.len_utf16();
    }
    if utf16_len > 255 {
        return Err(FsError::UnsupportedName);
    }
    Ok(())
}

fn encode_lfn_entries(name: &str, name83: &[u8; 11]) -> Result<Vec<[u8; DIR_ENTRY_SIZE]>, FsError> {
    let utf16: Vec<u16> = name.encode_utf16().collect();
    if utf16.is_empty() || utf16.len() > 255 {
        return Err(FsError::UnsupportedName);
    }

    let checksum = lfn_checksum(name83);
    let count = (utf16.len() + 12) / 13;
    let mut entries = Vec::new();
    for seq in (1..=count).rev() {
        let start = (seq - 1) * 13;
        let end = (start + 13).min(utf16.len());
        let chars = &utf16[start..end];
        let mut entry = [0u8; DIR_ENTRY_SIZE];
        entry[0] = seq as u8;
        if seq == count {
            entry[0] |= 0x40;
        }
        entry[11] = ATTR_LFN;
        entry[13] = checksum;
        for (i, &off) in LFN_CHAR_OFFSETS.iter().enumerate() {
            let value = if i < chars.len() {
                chars[i]
            } else if i == chars.len() {
                0x0000
            } else {
                0xFFFF
            };
            entry[off..off + 2].copy_from_slice(&value.to_le_bytes());
        }
        entries.push(entry);
    }
    Ok(entries)
}

// ── Public API ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct DirEntryInfo {
    pub name: String,
    pub is_dir: bool,
    pub size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct FsStats {
    pub total_clusters: u32,
    pub free_clusters: u32,
    pub used_clusters: u32,
    pub bytes_per_cluster: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct FsCheckReport {
    pub ok: bool,
    pub root_entries: usize,
    pub stats: FsStats,
}

pub fn stats() -> Option<FsStats> {
    let bpb = Bpb::load()?;
    let total_entries = bpb.fat_entry_count();
    let mut free_clusters = 0u32;
    let mut used_clusters = 0u32;
    let mut sec = [0u8; SECTOR_SIZE];
    let entries_per_sector = bpb.bytes_per_sector as usize / 4;

    for sector_index in 0..bpb.sectors_per_fat {
        if !crate::ata::read_sector(bpb.fat_start_lba() + sector_index, &mut sec) {
            return None;
        }
        for entry_idx in 0..entries_per_sector {
            let cluster = sector_index * entries_per_sector as u32 + entry_idx as u32;
            if cluster < 2 || cluster >= total_entries {
                continue;
            }
            let off = entry_idx * 4;
            let value = u32::from_le_bytes([sec[off], sec[off + 1], sec[off + 2], sec[off + 3]])
                & FAT_ENTRY_MASK;
            if value == FAT_FREE {
                free_clusters += 1;
            } else {
                used_clusters += 1;
            }
        }
    }

    Some(FsStats {
        total_clusters: total_entries.saturating_sub(2),
        free_clusters,
        used_clusters,
        bytes_per_cluster: bpb.sectors_per_cluster as u32 * bpb.bytes_per_sector as u32,
    })
}

pub fn check() -> Option<FsCheckReport> {
    let root_entries = list_dir("/")?.len();
    let stats = stats()?;
    Some(FsCheckReport {
        ok: stats.free_clusters + stats.used_clusters == stats.total_clusters,
        root_entries,
        stats,
    })
}

pub fn list_dir(path: &str) -> Option<Vec<DirEntryInfo>> {
    let bpb = Bpb::load()?;
    let cluster = resolve_dir_cluster(&bpb, path).ok()?;

    let sectors = bpb.cluster_chain_sectors(cluster);
    let mut buf = [0u8; SECTOR_SIZE];
    let mut entries = Vec::new();
    let mut lfn_fragments: Vec<LfnFragment> = Vec::new();
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return None;
        }
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            let slot = &buf[offset..offset + DIR_ENTRY_SIZE];
            if slot[0] == 0x00 {
                return Some(entries);
            }
            if slot[0] == 0xE5 {
                lfn_fragments.clear();
                continue;
            }
            if slot[11] == ATTR_LFN {
                if let Some(fragment) = read_lfn_fragment(slot) {
                    lfn_fragments.push(fragment);
                } else {
                    lfn_fragments.clear();
                }
                continue;
            }
            if let Some(entry) = DirEntry::from_bytes(slot) {
                let name = assemble_lfn_for_entry(&entry, &lfn_fragments)
                    .unwrap_or_else(|| entry.name_as_string());
                lfn_fragments.clear();
                if name == "." || name == ".." {
                    continue;
                }
                entries.push(DirEntryInfo {
                    name,
                    is_dir: entry.is_dir(),
                    size: entry.file_size,
                });
            } else {
                lfn_fragments.clear();
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
    let (bpb, parent_cluster, fat_name) = prepare_create(path)?;
    let mut entries = fat_name.lfn_entries;
    entries.push(encode_dir_entry(fat_name.short, ATTR_ARCHIVE, 0, 0));
    append_dir_entries(&bpb, parent_cluster, &entries)
}

pub fn create_dir(path: &str) -> Result<(), FsError> {
    let (bpb, parent_cluster, fat_name) = prepare_create(path)?;
    let new_cluster = alloc_cluster(&bpb)?;

    if let Err(err) = init_directory_cluster(&bpb, new_cluster, parent_cluster) {
        let _ = free_single_cluster(&bpb, new_cluster);
        return Err(err);
    }

    let mut entries = fat_name.lfn_entries;
    entries.push(encode_dir_entry(
        fat_name.short,
        ATTR_DIRECTORY,
        new_cluster,
        0,
    ));
    if let Err(err) = append_dir_entries(&bpb, parent_cluster, &entries) {
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
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    let location =
        find_entry_location(&bpb, parent_cluster, old_name.as_bytes()).ok_or(FsError::NotFound)?;
    if let Some(existing) = find_entry_location(&bpb, parent_cluster, new_name.as_bytes()) {
        if existing.lba != location.lba || existing.offset != location.offset {
            return Err(FsError::AlreadyExists);
        }
    }

    let fat_name = encode_name_for_dir(
        &bpb,
        parent_cluster,
        new_name,
        Some((location.lba, location.offset)),
    )?;
    let mut short_entry = read_dir_slot(location.lba, location.offset)?;
    short_entry[0..11].copy_from_slice(&fat_name.short);

    if fat_name.lfn_entries.is_empty() {
        write_dir_slot(location.lba, location.offset, &short_entry)?;
        for &(lba, offset) in &location.lfn_locations {
            mark_dir_slot_deleted(lba, offset)?;
        }
        return Ok(());
    }

    if location.lfn_locations.len() == fat_name.lfn_entries.len() {
        for (entry, &(lba, offset)) in fat_name
            .lfn_entries
            .iter()
            .zip(location.lfn_locations.iter())
        {
            write_dir_slot(lba, offset, entry)?;
        }
        write_dir_slot(location.lba, location.offset, &short_entry)?;
        return Ok(());
    }

    let mut entries = fat_name.lfn_entries;
    entries.push(short_entry);
    append_dir_entries(&bpb, parent_cluster, &entries)?;
    mark_entry_location_deleted(&location)
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

pub fn delete_file(path: &str) -> Result<(), FsError> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    let location =
        find_entry_location(&bpb, parent_cluster, name.as_bytes()).ok_or(FsError::NotFound)?;
    if location.entry.is_dir() {
        let dir_cluster = location.entry.first_cluster();
        let children = list_dir(path).unwrap_or_default();
        if !children.is_empty() {
            return Err(FsError::NotEmpty);
        }
        if dir_cluster >= 2 {
            free_cluster_chain(&bpb, dir_cluster)?;
        }
    } else {
        let first_cluster = location.entry.first_cluster();
        if first_cluster >= 2 {
            free_cluster_chain(&bpb, first_cluster)?;
        }
    }
    mark_entry_location_deleted(&location)
}

pub fn copy_file(src: &str, dst: &str) -> Result<(), FsError> {
    let data = read_file(src).ok_or(FsError::NotFound)?;
    create_file(dst)?;
    write_file(dst, &data)
}

pub fn safe_write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
    let (parent, name) = split_parent_and_name(path)?;
    let mut tmp = parent.clone();
    if !tmp.ends_with('/') {
        tmp.push('/');
    }
    tmp.push_str("CWTMP.TMP");

    match delete_file(&tmp) {
        Ok(()) | Err(FsError::NotFound) => {}
        Err(err) => return Err(err),
    }
    create_file(&tmp)?;
    write_file(&tmp, data)?;
    match delete_file(path) {
        Ok(()) | Err(FsError::NotFound) => {}
        Err(err) => {
            let _ = delete_file(&tmp);
            return Err(err);
        }
    }
    rename(&tmp, &name)
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

fn prepare_create(path: &str) -> Result<(Bpb, u32, FatName), FsError> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let bpb = Bpb::load().ok_or(FsError::Io)?;
    let parent_cluster = resolve_dir_cluster(&bpb, &parent_path)?;
    if find_entry(&bpb, parent_cluster, name.as_bytes()).is_some() {
        return Err(FsError::AlreadyExists);
    }
    let fat_name = encode_name_for_dir(&bpb, parent_cluster, &name, None)?;
    Ok((bpb, parent_cluster, fat_name))
}

fn encode_name_for_dir(
    bpb: &Bpb,
    dir_cluster: u32,
    name: &str,
    exclude: Option<(u32, usize)>,
) -> Result<FatName, FsError> {
    if let Ok(short) = name_to_83_checked(name) {
        if short_name_exists_except(bpb, dir_cluster, &short, exclude) {
            return Err(FsError::AlreadyExists);
        }
        return Ok(FatName {
            short,
            lfn_entries: Vec::new(),
        });
    }

    validate_lfn_name(name)?;
    let short = generate_short_alias(bpb, dir_cluster, name, exclude)?;
    let lfn_entries = encode_lfn_entries(name, &short)?;
    Ok(FatName { short, lfn_entries })
}

fn generate_short_alias(
    bpb: &Bpb,
    dir_cluster: u32,
    name: &str,
    exclude: Option<(u32, usize)>,
) -> Result<[u8; 11], FsError> {
    let (stem, ext) = split_lfn_stem_ext(name);
    let mut base = sanitize_short_component(stem, b"FILE");
    let ext = sanitize_short_component(ext, b"");
    if base.is_empty() {
        base.extend_from_slice(b"FILE");
    }

    for counter in 1..1_000_000u32 {
        let mut suffix = String::from("~");
        push_decimal(&mut suffix, counter);
        if suffix.len() >= 8 {
            continue;
        }

        let prefix_len = base.len().min(8 - suffix.len());
        let mut candidate = [b' '; 11];
        for (i, &b) in base.iter().take(prefix_len).enumerate() {
            candidate[i] = b;
        }
        for (i, &b) in suffix.as_bytes().iter().enumerate() {
            candidate[prefix_len + i] = b;
        }
        for (i, &b) in ext.iter().take(3).enumerate() {
            candidate[8 + i] = b;
        }

        if !short_name_exists_except(bpb, dir_cluster, &candidate, exclude) {
            return Ok(candidate);
        }
    }

    Err(FsError::NoSpace)
}

fn split_lfn_stem_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(0) | None => (name, ""),
        Some(pos) if pos + 1 < name.len() => (&name[..pos], &name[pos + 1..]),
        Some(_) => (name, ""),
    }
}

fn sanitize_short_component(component: &str, fallback: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for b in component.bytes() {
        let upper = b.to_ascii_uppercase();
        if is_valid_short_name_char(upper) {
            out.push(upper);
        }
    }
    if out.is_empty() {
        out.extend_from_slice(fallback);
    }
    out
}

fn push_decimal(out: &mut String, mut value: u32) {
    let mut digits = [0u8; 10];
    let mut len = 0usize;
    loop {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for &digit in digits[..len].iter().rev() {
        out.push(digit as char);
    }
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
    let mut lfn_fragments: Vec<LfnFragment> = Vec::new();
    let mut lfn_locations: Vec<(u32, usize)> = Vec::new();
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return None;
        }
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            let slot = &buf[offset..offset + DIR_ENTRY_SIZE];
            if slot[0] == 0x00 {
                return None;
            }
            if slot[0] == 0xE5 {
                lfn_fragments.clear();
                lfn_locations.clear();
                continue;
            }
            if slot[11] == ATTR_LFN {
                if let Some(fragment) = read_lfn_fragment(slot) {
                    lfn_fragments.push(fragment);
                    lfn_locations.push((lba, offset));
                } else {
                    lfn_fragments.clear();
                    lfn_locations.clear();
                }
                continue;
            }
            if let Some(entry) = DirEntry::from_bytes(slot) {
                let lfn_name = assemble_lfn_for_entry(&entry, &lfn_fragments);
                let short_name_matches = entry.matches(name);
                let long_name_matches = lfn_name
                    .as_ref()
                    .map(|lfn_name| lfn_name_matches(lfn_name, name))
                    .unwrap_or(false);
                if short_name_matches || long_name_matches {
                    let locations = if lfn_name.is_some() {
                        lfn_locations.clone()
                    } else {
                        Vec::new()
                    };
                    return Some(DirEntryLocation {
                        entry,
                        lba,
                        offset,
                        lfn_locations: locations,
                    });
                }
            }
            lfn_fragments.clear();
            lfn_locations.clear();
        }
    }
    None
}

fn short_name_exists_except(
    bpb: &Bpb,
    dir_cluster: u32,
    name83: &[u8; 11],
    exclude: Option<(u32, usize)>,
) -> bool {
    let sectors = bpb.cluster_chain_sectors(dir_cluster);
    let mut buf = [0u8; SECTOR_SIZE];
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) {
            return true;
        }
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            let slot = &buf[offset..offset + DIR_ENTRY_SIZE];
            if slot[0] == 0x00 {
                return false;
            }
            if slot[0] == 0xE5 || slot[11] == ATTR_LFN {
                continue;
            }
            if let Some(entry) = DirEntry::from_bytes(slot) {
                let excluded = exclude
                    .map(|(exclude_lba, exclude_offset)| {
                        exclude_lba == lba && exclude_offset == offset
                    })
                    .unwrap_or(false);
                if !excluded && entry.name == *name83 {
                    return true;
                }
            }
        }
    }
    false
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

fn append_dir_entries(
    bpb: &Bpb,
    dir_cluster: u32,
    entries: &[[u8; DIR_ENTRY_SIZE]],
) -> Result<(), FsError> {
    if entries.is_empty() {
        return Ok(());
    }

    loop {
        if let Some((locations, needs_end_marker)) =
            find_free_dir_entry_run(bpb, dir_cluster, entries.len())?
        {
            for (entry, &(lba, offset)) in entries.iter().zip(locations.iter()) {
                write_dir_slot(lba, offset, entry)?;
            }
            if needs_end_marker {
                if let Some(&(lba, offset)) = locations.last() {
                    write_dir_end_marker_after(bpb, dir_cluster, lba, offset)?;
                }
            }
            return Ok(());
        }

        extend_dir_chain(bpb, dir_cluster)?;
    }
}

fn find_free_dir_entry_run(
    bpb: &Bpb,
    dir_cluster: u32,
    needed: usize,
) -> Result<Option<(Vec<(u32, usize)>, bool)>, FsError> {
    let sectors = bpb.cluster_chain_sectors(dir_cluster);
    let mut sec = [0u8; SECTOR_SIZE];
    let mut run: Vec<(u32, usize)> = Vec::new();
    let mut run_hit_end = false;

    for lba in sectors {
        read_sector_exact(lba, &mut sec)?;
        for offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            let first = sec[offset];
            if first == 0xE5 || first == 0x00 {
                if run.is_empty() {
                    run_hit_end = false;
                }
                if first == 0x00 {
                    run_hit_end = true;
                }
                run.push((lba, offset));
                if run.len() == needed {
                    return Ok(Some((run, run_hit_end)));
                }
            } else {
                run.clear();
                run_hit_end = false;
            }
        }
    }

    Ok(None)
}

fn extend_dir_chain(bpb: &Bpb, dir_cluster: u32) -> Result<(), FsError> {
    let clusters = bpb.cluster_chain_clusters(dir_cluster);
    let last_cluster = *clusters.last().ok_or(FsError::Io)?;
    let new_cluster = alloc_cluster(bpb)?;
    if let Err(err) = fat_write_entry(bpb, last_cluster, new_cluster) {
        let _ = free_single_cluster(bpb, new_cluster);
        return Err(err);
    }
    Ok(())
}

fn next_dir_slot(bpb: &Bpb, dir_cluster: u32, lba: u32, offset: usize) -> Option<(u32, usize)> {
    let mut return_next = false;
    for sector_lba in bpb.cluster_chain_sectors(dir_cluster) {
        for sector_offset in (0..SECTOR_SIZE).step_by(DIR_ENTRY_SIZE) {
            if return_next {
                return Some((sector_lba, sector_offset));
            }
            if sector_lba == lba && sector_offset == offset {
                return_next = true;
            }
        }
    }
    None
}

fn read_dir_slot(lba: u32, offset: usize) -> Result<[u8; DIR_ENTRY_SIZE], FsError> {
    let mut sec = [0u8; SECTOR_SIZE];
    read_sector_exact(lba, &mut sec)?;
    let mut entry = [0u8; DIR_ENTRY_SIZE];
    entry.copy_from_slice(&sec[offset..offset + DIR_ENTRY_SIZE]);
    Ok(entry)
}

fn write_dir_slot(lba: u32, offset: usize, entry: &[u8; DIR_ENTRY_SIZE]) -> Result<(), FsError> {
    let mut sec = [0u8; SECTOR_SIZE];
    read_sector_exact(lba, &mut sec)?;
    sec[offset..offset + DIR_ENTRY_SIZE].copy_from_slice(entry);
    write_sector_exact(lba, &sec)
}

fn mark_dir_slot_deleted(lba: u32, offset: usize) -> Result<(), FsError> {
    let mut sec = [0u8; SECTOR_SIZE];
    read_sector_exact(lba, &mut sec)?;
    sec[offset] = 0xE5;
    write_sector_exact(lba, &sec)
}

fn mark_entry_location_deleted(location: &DirEntryLocation) -> Result<(), FsError> {
    for &(lba, offset) in &location.lfn_locations {
        mark_dir_slot_deleted(lba, offset)?;
    }
    mark_dir_slot_deleted(location.lba, location.offset)
}

fn write_dir_end_marker_after(
    bpb: &Bpb,
    dir_cluster: u32,
    lba: u32,
    offset: usize,
) -> Result<(), FsError> {
    if let Some((next_lba, next_offset)) = next_dir_slot(bpb, dir_cluster, lba, offset) {
        let mut sec = [0u8; SECTOR_SIZE];
        read_sector_exact(next_lba, &mut sec)?;
        sec[next_offset] = 0x00;
        write_sector_exact(next_lba, &sec)?;
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
