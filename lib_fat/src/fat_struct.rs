use std::fmt;

/// FAT type
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum FatType {
    Fat12,
    Fat16,
    Fat32,
}
impl fmt::Display for FatType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            FatType::Fat12 => write!(f, "FAT12"),
            FatType::Fat16 => write!(f, "FAT16"),
            FatType::Fat32 => write!(f, "FAT32"),
        }
    }
}

/// Boot sector
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatBs {
    // 0: Jump boot code
    pub(crate) jump: [u8; 3],
    // 3: OEM identifier
    pub(crate) oem_name: [u8; 8],
}

/// BIOS parameter block
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatBpb {
    // B: Bytes per sector, 512/1024/2048/4096
    pub(crate) bytes_per_sector: u16,
    // D: Sectors per cluster - 1/2/4/8/16/32/64
    pub(crate) sectors_per_cluster: u8,
    // E: Number of reserved clusters
    pub(crate) reserved_clusters: u16,
    // 10: Number of FAT tables
    pub(crate) num_fats: u8,
    // 11: Root directory entries
    pub(crate) root_entry_count: u16,
    // 13: Total sectors in logical volume
    pub(crate) total_sectors_16: u16,
    // 15: Media descriptor
    pub(crate) media_descriptor: u8,
    // 16: Sectors per FAT
    pub(crate) fat_size_16: u16,

    // 18: Sectors per track
    pub(crate) sectors_per_track: u16,
    // 1A: Number of heads
    pub(crate) heads: u16,
    // 1C: Number of hidden sectors
    pub(crate) hidden_sectors_count: u32,

    // 20: Total logical sectors
    pub(crate) total_sectors_32: u32,
}

/// Extended BIOS parameter block (FAT12/FAT16)
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatEbpb {
    // 24: Drive number
    pub(crate) drive_number: u8,
    // 25: Reserved
    pub(crate) reserved: u8,
    // 26: Extended boot signature: 0x28 or 0x29 *
    pub(crate) boot_signature: u8,
    // 27: Volume ID (serial number)
    pub(crate) volume_id: [u8; 4],
    // 20: Partition volume label
    pub(crate) volume_label: [u8; 11],
    // 2B: Filesystem type, padded with blanks
    pub(crate) fs_type: [u8; 8],
}

/// FAT32 EBPB
#[allow(dead_code)]
#[derive(Debug)]
pub struct Fat32Ebpb {
    // 24: Logical sectors per FAT
    pub(crate) fat_size_32: u32,
    // 28: Drive description/mirroring flags
    pub(crate) flags: u16,
    // 2A: Version
    pub(crate) version: u16,
    // 2C: Cluster number of root directory
    pub(crate) root_cluster: u32,
    // 30: Sector number of FS Information Sector
    pub(crate) fsinfo_sector: u16,
    // 32: Sector number of backup boot sector
    pub(crate) backup_sector: u16,
    // 34: Reserved
    pub(crate) reserved: [u8; 12],
    // 40: Physical drive number
    pub(crate) drive_number: u8,
    // 41: Reserved flags
    pub(crate) reserved_flags: u8,
    // 42: Signature - 0x28/0x29 *
    pub(crate) signature: u8,
    // 43: Volume ID (serial number)
    pub(crate) volume_id: [u8; 4],
    // 47: Partition volume label
    pub(crate) volume_label: [u8; 11],
    // 52: Filesystem type, padded with blanks
    pub(crate) fs_type: [u8; 8],
}

/// FAT32 FSInfo
#[allow(dead_code)]
pub struct Fat32FsInfo {
    // 0: Lead signature
    pub(crate) lead_signature: u32,
    // 4: Reserved, should be initialised to 0
    pub(crate) reserved: [u8; 480],
    // 1E4: Struct sig
    pub(crate) struct_sig: u32,
    // 1E8: Free count
    pub(crate) free_count: u32,
    // 1EC: Next free (where to start looking)
    pub(crate) next_free: u32,
    // 1F0: Reserved 2
    pub(crate) reserved2: [u8; 12],
    // 1FC: Trail signature - validate FsInfo sector
    pub(crate) trail_signature: u32,
}

/// FAT directory structure
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatDirectoryEntry {
    // 0: Short name
    // If name[0]==0xE5, entry is free
    // If name[0]==0x00, entry is free and no allocated entries after this one
    pub(crate) name: [u8; 11],
    // B: Dir Attr
    pub(crate) attribute: u8,
    // C: NT reserved
    pub(crate) nt_reserved: u8,
    // D: Stamp at file creation date (0-199 inclusive)
    pub(crate) created_time_tenth: u8,
    // E: Created time
    pub(crate) created_time: u16,
    // 10: Created date
    pub(crate) created_date: u16,
    // 12: Last accessed date
    pub(crate) last_accessed: u16,
    // 14: High word of entry's first cluster number
    pub(crate) first_cluster_hi: u16,
    // 16: Write time
    pub(crate) write_time: u16,
    // 18: Write date
    pub(crate) write_date: u16,
    // 1A: Low word of entry's first cluster number
    pub(crate) first_cluster_low: u16,
    // 1C: File size
    pub(crate) size: u32,
}

/// Enum for file types
pub enum FatFileType {
    AttrReadOnly = 0x01,
    AttrHidden = 0x02,
    AttrSystem = 0x04,
    AttrVolumeId = 0x08,
    AttrDirectory = 0x10,
    AttrArchive = 0x20,
    AttrLongname = 0x01 | 0x02 | 0x04 | 0x08,
}

/// FAT long directory structure
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatLongDirectoryEntry {
    // 0: Order
    pub(crate) order: u8,
    // 1: Characters 1-5
    pub(crate) name1: [u16; 5],
    // B: Attributes - must be ATTR_LONG_NAME
    pub(crate) attr: u8,
    // C: Type
    pub(crate) dir_type: u8,
    // D: Checksum
    pub(crate) checksum: u8,
    // E: Characters 6-11
    pub(crate) name2: [u16; 6],
    // 1A: First cluster low -> must be 0
    pub(crate) first_cluster_low: u16,
    // 1C: Characters 12-13
    pub(crate) name3: [u16; 2],
}

/// FAT directory entry container
/// For file, there must be 1 short entry and possibly multiple long entries
#[allow(dead_code)]
#[derive(Debug)]
pub struct FatDirectoryEntryContainer {
    pub(crate) short_entry: FatDirectoryEntry,
    pub(crate) long_entries: Vec<FatLongDirectoryEntry>,
    pub(crate) cached_name: String,
    pub(crate) cached_cluster_count: u32,
}
