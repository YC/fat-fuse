use byteorder::{LittleEndian, ReadBytesExt};
use std::char::{decode_utf16, REPLACEMENT_CHARACTER};
use std::convert::TryInto;
use std::io::{Cursor, Read};

use super::{
    file_cluster_count, read_file_full, read_sector, root_dir_sectors, Fat,
    FatDirectoryEntry, FatDirectoryEntryContainer, FatFileType,
    FatLongDirectoryEntry,
    FatType,
};

/// Reads/loads root directory
pub fn read_root_dir(fat: &mut Fat) {
    match fat.fat_type {
        FatType::Fat12 | FatType::Fat16 => {
            // Fixed location on disk following last FAT
            let first_root_sector_num: u16 = fat.bpb.reserved_clusters
                + (fat.bpb.num_fats as u16 * fat.bpb.fat_size_16);
            // Find number of sectors occupied by root directory
            let root_sector_count = root_dir_sectors(fat);
            // Read root dir sectors to u8 vector
            let mut root_dir: Vec<u8> = Vec::new();
            for i in 0..root_sector_count {
                root_dir.extend(read_sector(
                    fat,
                    (i + first_root_sector_num).into(),
                ));
            }
            // Read root dir and assign root cluster number of 0
            read_dir_chain(fat, 0, &root_dir, 0);
        }
        FatType::Fat32 => {
            // For FAT32, treat root directory as file
            let root_cluster = fat.ebpb32.as_ref().unwrap().root_cluster;
            let root_dir = read_file_full(fat, root_cluster);
            read_dir_chain(fat, root_cluster, &root_dir, 0);
        }
    }
}

/// Retrieves the directory with specified inode
pub fn get_dir(
    fat: &mut Fat,
    inode: u32,
) -> Option<&Vec<FatDirectoryEntryContainer>> {
    let cached = fat.dir_cache.contains_key(&inode);
    if !cached {
        let dir_file = read_file_full(fat, inode);
        read_dir_chain(fat, inode, &dir_file, 0);
    }
    fat.dir_cache.get(&inode)
}

/// Reads a chain of directory entries
pub fn read_dir_chain(fat: &mut Fat, inode: u32, sector: &[u8], start: u16) {
    // Directory containers for entries
    let mut directory_entries: Vec<FatDirectoryEntryContainer> = vec![];

    // Read beginning
    let mut current: usize = start.into();
    let mut current_buffer: &[u8];
    let mut current_long_entries: Vec<FatLongDirectoryEntry> = vec![];

    loop {
        // Read entry at offset
        current_buffer = &sector[current..current + 32];
        // Last entry
        if current_buffer[0] == 0x0 {
            break;
        }
        // Free entry
        if current_buffer[0] == 0xE5 {
            current += 32;
            continue;
        }

        let attr_long_name_mask = FatFileType::AttrReadOnly as u8
            | FatFileType::AttrHidden as u8
            | FatFileType::AttrSystem as u8
            | FatFileType::AttrVolumeId as u8
            | FatFileType::AttrDirectory as u8
            | FatFileType::AttrArchive as u8;
        let attr = current_buffer[11];
        if attr & attr_long_name_mask == FatFileType::AttrLongname as u8 {
            // Long entry
            current_long_entries
                .push(FatLongDirectoryEntry::new(current_buffer));
        } else {
            let test_val = FatFileType::AttrDirectory as u8
                | FatFileType::AttrVolumeId as u8;

            if attr & test_val == 0
                || attr & test_val == FatFileType::AttrDirectory as u8
                || attr & test_val == FatFileType::AttrVolumeId as u8
            {
                // Short entry
                let short_entry = FatDirectoryEntry::new(current_buffer);
                let checksum = chksum(&short_entry.name);

                // Move long entries
                let mut long_entries: Vec<FatLongDirectoryEntry> = vec![];
                while !current_long_entries.is_empty() {
                    let current_long_entry = current_long_entries.remove(0);
                    // Ensure that N is correct
                    let n: u8 = current_long_entries.len() as u8 + 1;
                    if current_long_entry.attr & n == 0 {
                        long_entries = vec![];
                        break;
                    }
                    // Ensure that checksum is correct
                    if current_long_entry.checksum != checksum {
                        long_entries = vec![];
                        break;
                    }

                    // Correct, so push
                    long_entries.push(current_long_entry);
                }

                // Determine cluster count
                let cluster_count =
                    file_cluster_count(fat, short_entry.cluster_number());
                // Parse name
                let name = FatDirectoryEntryContainer::parse_name(
                    &short_entry,
                    &long_entries,
                );
                // Merge into directory container
                directory_entries.push(FatDirectoryEntryContainer {
                    short_entry,
                    long_entries,
                    cached_name: name,
                    cached_cluster_count: cluster_count,
                });
            }
        }
        current += 32;
        continue;
    }

    // Cache parents
    for entry in directory_entries.iter() {
        fat.inode_cache.insert(entry.cluster_number(), inode);
    }
    // Cache entries
    fat.dir_cache.insert(inode, directory_entries);
}

/// Calculates checksum of short name
fn chksum(name: &[u8]) -> u8 {
    let mut sum: u8 = 0;
    for c in name.iter() {
        let p1: u8 = if sum & 1 != 0 { 0x80 } else { 0 };
        let (p2, _) = sum.overflowing_shr(1);

        let (a1, _) = p1.overflowing_add(p2);
        let (a2, _) = a1.overflowing_add(*c);
        sum = a2;
    }
    sum
}

impl FatDirectoryEntry {
    /// Combine low/hi fields to get cluster number (2 WORDs to a DWORD)
    pub fn cluster_number(&self) -> u32 {
        (self.first_cluster_hi as u32) << 16 | self.first_cluster_low as u32
    }
}

impl FatDirectoryEntryContainer {
    /// Get attribute
    pub fn attribute(&self) -> u8 {
        self.short_entry.attribute
    }

    /// Get size
    pub fn size(&self) -> u32 {
        self.short_entry.size
    }

    /// Get cluster number
    pub fn cluster_number(&self) -> u32 {
        self.short_entry.cluster_number()
    }

    /// Get cluster count of file
    pub fn cluster_count(&self, is_fat32: bool) -> u32 {
        // Root dir for FAT12/16
        if self.cluster_number() == 0 && !is_fat32 {
            return 1;
        }
        self.cached_cluster_count
    }

    /// Returns properly formatted name of directory/file
    pub fn get_name(&self) -> &String {
        &self.cached_name
    }

    /// Get creation time
    pub fn get_creation_time(&self) -> (u16, u8, u8, u8, u8, u16) {
        let (year, month, day) = parse_date(self.short_entry.created_date);
        let (hour, minute, second) = parse_time(self.short_entry.created_time);
        let _ms = self.short_entry.created_time_tenth;
        (year, month, day, hour, minute, second as u16)
    }

    /// Get last accessed date
    pub fn get_last_accessed_date(&self) -> (u16, u8, u8) {
        parse_date(self.short_entry.last_accessed)
    }

    /// Get write time
    pub fn get_write_time(&self) -> (u16, u8, u8, u8, u8, u8) {
        let (year, month, day) = parse_date(self.short_entry.write_date);
        let (hour, minute, second) = parse_time(self.short_entry.write_time);
        (year, month, day, hour, minute, second)
    }

    /// Parses name into string
    fn parse_name(
        short_entry: &FatDirectoryEntry,
        long_entries: &[FatLongDirectoryEntry],
    ) -> String {
        match long_entries.len() {
            0 => {
                let mut buf = vec![];
                let name_bytes = short_entry.name;
                let name = &name_bytes[1..8];
                let ext = &name_bytes[8..11];

                // Special case for 0x05 -> 0xE5 for first character
                match name_bytes[0] {
                    0x05 => buf.push(0xE5),
                    c => buf.push(c),
                };

                // Process the name
                let mut end_index = 0;
                // Find last character that is not 0x20
                for (index, c) in name.iter().rev().enumerate() {
                    if *c != 0x20 {
                        end_index = name.len() - index;
                        break;
                    }
                }
                // Add that section
                if end_index != 0 {
                    buf.extend(&name[0..end_index]);
                }

                // Process the ext
                if ext[0] != 0x20 {
                    // Push a . and first character of extension
                    buf.push(0x2E);
                    buf.push(ext[0]);

                    // Push rest of extension as necessary
                    if ext[2] != 0x20 {
                        buf.push(ext[1]);
                        buf.push(ext[2]);
                    } else if ext[1] != 0x20 {
                        buf.push(ext[1]);
                    }
                }

                // Should technically also do a check for illegal characters...
                String::from_utf8(buf).unwrap()
            }
            _ => {
                // Declare array
                let num_long_entries = long_entries.len();
                let mut name_bytes = vec![0; num_long_entries * 13];

                // Add various portions of the name
                for entry in long_entries.iter() {
                    let entry_n = entry.order & !0x40;
                    let offset: usize = (entry_n as usize - 1) * 13;
                    replace_vec_section(&mut name_bytes, &entry.name1, offset);
                    replace_vec_section(
                        &mut name_bytes,
                        &entry.name2,
                        offset + 5,
                    );
                    replace_vec_section(
                        &mut name_bytes,
                        &entry.name3,
                        offset + 11,
                    );
                }

                // Find terminator and take slice
                let index = name_bytes.iter().position(|&r| r == 0).unwrap();
                let name: Vec<u16> = name_bytes[0..index].to_vec();
                // To string
                decode_utf16(name)
                    .map(|r| r.unwrap_or(REPLACEMENT_CHARACTER))
                    .collect::<String>()
            }
        }
    }
}

/// Parses FAT directory entry date stamp to (year, month, day) tuple
fn parse_date(date: u16) -> (u16, u8, u8) {
    let day = date & (0b0000000000011111);
    let month = (date & (0b0000000111100000)) >> 5;
    let year = ((date & (0b1111111000000000)) >> 9) + 1980;
    (year, month.try_into().unwrap(), day.try_into().unwrap())
}

/// Parses FAT directory entry time stamp to (hour, minute, second) tuple
fn parse_time(time: u16) -> (u8, u8, u8) {
    let second = (time & (0b0000000000011111)) * 2;
    let minute = (time & (0b0000011111100000)) >> 5;
    let hour = (time & (0b1111100000000000)) >> 11;
    (
        hour.try_into().unwrap(),
        minute.try_into().unwrap(),
        second.try_into().unwrap(),
    )
}

/// Overwrites section of vector starting at 'start' with contents of array
fn replace_vec_section(v: &mut Vec<u16>, a: &[u16], start: usize) {
    for (index, c) in a.iter().enumerate() {
        v[start + index] = *c;
    }
}

/// Reads directory entry
/// Directory entries should be 32 bytes long
impl FatDirectoryEntry {
    fn new(entry_bytes: &[u8]) -> FatDirectoryEntry {
        // Create reader
        let mut cursor = Cursor::new(&entry_bytes);

        let mut name: [u8; 11] = Default::default();
        cursor.read_exact(&mut name).unwrap();
        let attribute: u8 = cursor.read_u8().unwrap();
        let nt_reserved: u8 = cursor.read_u8().unwrap();
        let created_time_tenth: u8 = cursor.read_u8().unwrap();
        let created_time: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let created_date: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let last_accessed: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let first_cluster_hi: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let write_time: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let write_date: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let first_cluster_low: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let size: u32 = cursor.read_u32::<LittleEndian>().unwrap();

        FatDirectoryEntry {
            name,
            attribute,
            nt_reserved,
            created_time_tenth,
            created_time,
            created_date,
            last_accessed,
            first_cluster_hi,
            write_time,
            write_date,
            first_cluster_low,
            size,
        }
    }
}

/// Reads long directory entry
impl FatLongDirectoryEntry {
    fn new(entry_bytes: &[u8]) -> FatLongDirectoryEntry {
        // Create reader
        let mut cursor = Cursor::new(&entry_bytes);

        let order: u8 = cursor.read_u8().unwrap();
        let mut name1: [u16; 5] = Default::default();
        cursor.read_u16_into::<LittleEndian>(&mut name1).unwrap();
        let attr: u8 = cursor.read_u8().unwrap();
        let dir_type: u8 = cursor.read_u8().unwrap();
        let checksum: u8 = cursor.read_u8().unwrap();
        let mut name2: [u16; 6] = Default::default();
        cursor.read_u16_into::<LittleEndian>(&mut name2).unwrap();
        let first_cluster_low: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let mut name3: [u16; 2] = Default::default();
        cursor.read_u16_into::<LittleEndian>(&mut name3).unwrap();

        FatLongDirectoryEntry {
            order,
            name1,
            attr,
            dir_type,
            checksum,
            name2,
            first_cluster_low,
            name3,
        }
    }
}
