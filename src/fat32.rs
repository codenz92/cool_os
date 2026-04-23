/// Read-only FAT32 filesystem parser (Phase 11).
///
/// Supports 8.3 filenames only.  Reads sector-by-sector via `crate::ata`.
/// All multi-byte fields are little-endian (x86).

extern crate alloc;
use alloc::vec::Vec;

// ── BIOS Parameter Block (BPB) ────────────────────────────────────────────────

/// Parsed FAT32 BPB fields we need for navigation.
#[derive(Debug, Clone, Copy)]
pub struct Bpb {
    pub bytes_per_sector:    u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors:    u16,
    pub num_fats:            u8,
    pub sectors_per_fat:     u32,
    pub root_cluster:        u32,
}

impl Bpb {
    pub fn load() -> Option<Self> {
        let mut sec = [0u8; 512];
        if !crate::ata::read_sector(0, &mut sec) {
            crate::println!("[fat32] BPB read_sector failed");
            return None;
        }

        // Validate FAT boot-sector signature.
        if sec[510] != 0x55 || sec[511] != 0xAA {
            crate::println!(
                "[fat32] bad BPB sig: {:02x} {:02x}; first16={:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
                sec[510], sec[511],
                sec[0], sec[1], sec[2], sec[3], sec[4], sec[5], sec[6], sec[7],
                sec[8], sec[9], sec[10], sec[11], sec[12], sec[13], sec[14], sec[15],
            );
            return None;
        }

        let bytes_per_sector   = u16::from_le_bytes([sec[11], sec[12]]);
        let sectors_per_cluster = sec[13];
        let reserved_sectors   = u16::from_le_bytes([sec[14], sec[15]]);
        let num_fats           = sec[16];
        // FAT16 sectors_per_fat field is at offset 22; 0 means FAT32 uses ext.
        let fat16_spf          = u16::from_le_bytes([sec[22], sec[23]]);
        let sectors_per_fat    = if fat16_spf != 0 {
            fat16_spf as u32
        } else {
            u32::from_le_bytes([sec[36], sec[37], sec[38], sec[39]])
        };
        let root_cluster       = u32::from_le_bytes([sec[44], sec[45], sec[46], sec[47]]);

        Some(Bpb { bytes_per_sector, sectors_per_cluster, reserved_sectors,
                   num_fats, sectors_per_fat, root_cluster })
    }

    /// LBA of the first sector of the FAT.
    pub fn fat_start_lba(&self) -> u32 { self.reserved_sectors as u32 }

    /// LBA of the first data cluster (cluster 2).
    pub fn data_start_lba(&self) -> u32 {
        self.fat_start_lba()
            + self.num_fats as u32 * self.sectors_per_fat
    }

    /// LBA of the first sector of cluster `n`.
    pub fn cluster_lba(&self, cluster: u32) -> u32 {
        self.data_start_lba() + (cluster - 2) * self.sectors_per_cluster as u32
    }

    /// Read the FAT entry for `cluster`; returns the next cluster number.
    pub fn fat_next(&self, cluster: u32) -> Option<u32> {
        // Each FAT32 entry is 4 bytes; 512-byte sectors hold 128 entries.
        let byte_offset  = cluster * 4;
        let sector_index = byte_offset / self.bytes_per_sector as u32;
        let byte_in_sec  = (byte_offset % self.bytes_per_sector as u32) as usize;

        let lba = self.fat_start_lba() + sector_index;
        let mut sec = [0u8; 512];
        if !crate::ata::read_sector(lba, &mut sec) { return None; }
        let entry = u32::from_le_bytes([
            sec[byte_in_sec],
            sec[byte_in_sec + 1],
            sec[byte_in_sec + 2],
            sec[byte_in_sec + 3],
        ]) & 0x0FFF_FFFF;
        Some(entry)
    }

    /// Collect all sectors in the cluster chain starting at `start`.
    pub fn cluster_chain_sectors(&self, start: u32) -> Vec<u32> {
        let mut sectors = Vec::new();
        let mut cluster = start;
        loop {
            if cluster < 2 || cluster >= 0x0FFF_FFF8 { break; }
            let lba = self.cluster_lba(cluster);
            for i in 0..self.sectors_per_cluster as u32 {
                sectors.push(lba + i);
            }
            match self.fat_next(cluster) {
                Some(next) if next >= 0x0FFF_FFF8 => break, // EOC
                Some(next) if next < 2 => break,            // free/bad cluster
                Some(next) => cluster = next,
                None => break,
            }
        }
        sectors
    }
}

// ── Directory entry ───────────────────────────────────────────────────────────

const ATTR_DIRECTORY: u8 = 0x10;
const ATTR_LFN:       u8 = 0x0F;

struct DirEntry {
    name:       [u8; 11], // 8.3 name, space-padded, uppercase
    attr:       u8,
    first_cluster_hi: u16,
    first_cluster_lo: u16,
    file_size:  u32,
}

impl DirEntry {
    fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < 32 { return None; }
        if b[0] == 0x00 { return None; } // end of directory
        if b[0] == 0xE5 { return None; } // deleted entry
        let attr = b[11];
        if attr == ATTR_LFN { return None; } // skip LFN entries
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

    fn is_dir(&self) -> bool { self.attr & ATTR_DIRECTORY != 0 }

    fn name_as_string(&self) -> alloc::string::String {
        let name = self.name;
        let mut base_buf = [0u8; 8];
        let mut base_len = 0usize;
        for &b in &name[..8] {
            if b == b' ' { break; }
            base_buf[base_len] = b;
            base_len += 1;
        }
        let mut ext_buf = [0u8; 3];
        let mut ext_len = 0usize;
        for &b in &name[8..] {
            if b == b' ' { break; }
            ext_buf[ext_len] = b;
            ext_len += 1;
        }
        let base_str = core::str::from_utf8(&base_buf[..base_len]).unwrap_or("????????");
        let ext_str = core::str::from_utf8(&ext_buf[..ext_len]).unwrap_or("");
        if ext_str.is_empty() {
            alloc::string::String::from(base_str)
        } else {
            let mut s = alloc::string::String::from(base_str);
            s.push('.');
            s.push_str(ext_str);
            s
        }
    }

    /// Match this entry's 8.3 name against an uppercase 8.3 component.
    fn matches(&self, component: &[u8]) -> bool {
        self.name == name_to_83(component)
    }
}

/// Convert a `\0`-terminated path component (e.g. b"HELLO.TXT") to an 8.3
/// name array (space-padded, uppercase, no dot).
fn name_to_83(s: &[u8]) -> [u8; 11] {
    let mut out = [b' '; 11];
    let s = if s.last() == Some(&0) { &s[..s.len() - 1] } else { s };
    let dot = s.iter().rposition(|&b| b == b'.');
    let (base, ext) = match dot {
        Some(p) => (&s[..p], &s[p + 1..]),
        None    => (s, &b""[..]),
    };
    for (i, &b) in base.iter().take(8).enumerate() {
        out[i] = b.to_ascii_uppercase();
    }
    for (i, &b) in ext.iter().take(3).enumerate() {
        out[8 + i] = b.to_ascii_uppercase();
    }
    out
}

// ── Public API ────────────────────────────────────────────────────────────────

/// A directory entry exposed to the rest of the kernel.
#[derive(Debug, Clone)]
pub struct DirEntryInfo {
    pub name: alloc::string::String,
    pub is_dir: bool,
    pub size: u32,
}

/// List all entries in a directory given its absolute path (e.g. `/bin`).
/// Returns `None` if the path doesn't exist or isn't a directory.
pub fn list_dir(path: &str) -> Option<Vec<DirEntryInfo>> {
    let bpb = Bpb::load()?;
    let mut cluster = bpb.root_cluster;
    let components: Vec<&[u8]> = path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();

    for component in components {
        match find_in_dir(&bpb, cluster, component)? {
            (next_cluster, true) => cluster = next_cluster,
            (_next, false) => return None,
        }
    }

    let sectors = bpb.cluster_chain_sectors(cluster);
    let mut buf = [0u8; 512];
    let mut entries = Vec::new();
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) { return None; }
        for offset in (0..512).step_by(32) {
            if let Some(entry) = DirEntry::from_bytes(&buf[offset..]) {
                let name = entry.name_as_string();
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

/// Read the entire contents of an absolute path (e.g. `/bin/hello.txt`) from
/// the FAT32 filesystem on the ATA slave device.  Returns `None` on any error.
///
/// Only 8.3 filenames are supported.  The path must start with `/`.
pub fn read_file(path: &str) -> Option<Vec<u8>> {
    crate::println!("[fat32] read_file: loading BPB");
    let bpb = Bpb::load()?;
    crate::println!("[fat32] BPB loaded: bps={} spc={} fat_start={} data_start={} root_clust={}",
        bpb.bytes_per_sector, bpb.sectors_per_cluster,
        bpb.fat_start_lba(), bpb.data_start_lba(), bpb.root_cluster);

    // Walk path components, starting at root cluster.
    let mut cluster = bpb.root_cluster;
    let components: Vec<&[u8]> = path
        .as_bytes()
        .split(|&b| b == b'/')
        .filter(|c| !c.is_empty())
        .collect();

    if components.is_empty() { return None; }

    let last_idx = components.len() - 1;
    for (i, &component) in components.iter().enumerate() {
        let is_last = i == last_idx;
        match find_in_dir(&bpb, cluster, component)? {
            (_next_cluster, false) if is_last => {
                // Found a file at the last component — but we need size.
                // Re-scan to get the full entry.
                let entry = find_entry(&bpb, cluster, component)?;
                return read_clusters(&bpb, entry.first_cluster(), entry.file_size);
            }
            (next_cluster, true) if !is_last => {
                // Found a directory — descend.
                cluster = next_cluster;
            }
            (_next_cluster, false) if !is_last => return None, // file mid-path
            _ => return None,
        }
    }
    None
}

/// Find a named entry in the directory rooted at `dir_cluster`.
/// Returns `(first_cluster, is_dir)` or `None` if not found.
fn find_in_dir(bpb: &Bpb, dir_cluster: u32, name: &[u8]) -> Option<(u32, bool)> {
    let entry = find_entry(bpb, dir_cluster, name)?;
    Some((entry.first_cluster(), entry.is_dir()))
}

fn find_entry(bpb: &Bpb, dir_cluster: u32, name: &[u8]) -> Option<DirEntry> {
    let sectors = bpb.cluster_chain_sectors(dir_cluster);
    let mut buf = [0u8; 512];
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) { return None; }
        for offset in (0..512).step_by(32) {
            if let Some(entry) = DirEntry::from_bytes(&buf[offset..]) {
                if entry.matches(name) { return Some(entry); }
            } else if buf[offset] == 0x00 {
                return None; // end of directory
            }
        }
    }
    None
}

fn read_clusters(bpb: &Bpb, start: u32, size: u32) -> Option<Vec<u8>> {
    let sectors = bpb.cluster_chain_sectors(start);
    let mut data = Vec::with_capacity(size as usize);
    let mut buf = [0u8; 512];
    for lba in sectors {
        if !crate::ata::read_sector(lba, &mut buf) { return None; }
        data.extend_from_slice(&buf);
    }
    data.truncate(size as usize);
    Some(data)
}
