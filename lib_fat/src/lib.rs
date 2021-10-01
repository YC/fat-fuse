use std::collections::HashMap;
use std::fs::File;
use std::str;

mod fat_struct;
use fat_struct::{
    Fat32Ebpb, FatBpb, FatBs, FatDirectoryEntry, FatEbpb,
    FatLongDirectoryEntry, FatType,
};
pub use fat_struct::{FatDirectoryEntryContainer, FatFileType};

mod fat_reserved;
use fat_reserved::read_reserved;

mod fat_helper;
use fat_helper::{
    file_cluster_count, first_sector_of_cluster, read_file_full, read_sector,
    root_dir_sectors,
};

mod fat_dir;
use fat_dir::{get_dir, read_root_dir};

// Wrapper
#[derive(Debug)]
pub struct Fat {
    // Reserved sectors
    pub(crate) bs: FatBs,
    pub(crate) bpb: FatBpb,
    pub(crate) ebpb16: Option<FatEbpb>,
    pub(crate) ebpb32: Option<Fat32Ebpb>,

    // File
    pub(crate) image: File,
    // FAT
    pub(crate) fat: HashMap<u32, Vec<u8>>,
    // Caches directories, (inode, directory entries) of directory
    pub(crate) dir_cache: HashMap<u32, Vec<FatDirectoryEntryContainer>>,
    // Caches inode attr locations, (child inode, parent inode)
    pub(crate) inode_cache: HashMap<u32, u32>,

    // Determined/derived
    pub(crate) fat_type: FatType,
}

impl Fat {
    /// Mount FAT volume
    pub fn mount_volume(filename: &str) -> Fat {
        // Open file
        let f = File::open(filename).expect("Bad file");
        // Read reserved sectors
        let mut fat = read_reserved(f);
        // Read the root directory
        read_root_dir(&mut fat);
        return fat;
    }

    /// Get root dir cluster number
    pub fn get_root_cluster_number(&self) -> u32 {
        if self.fat_type == FatType::Fat32 {
            return self.ebpb32.as_ref().unwrap().root_cluster;
        } else {
            return 0;
        }
    }

    /// Get data
    pub fn get_data(
        &mut self,
        ino: u32,
        offset: u64,
        size: u32,
    ) -> Option<Vec<u8>> {
        if self.inode_cache.contains_key(&ino) == false {
            return None;
        }

        // Read whole file. TODO: seek...
        let data = read_file_full(self, ino);
        let head: usize = offset as usize;
        let mut tail: usize = head + size as usize;

        // Front is beyond length of data
        if offset as usize > data.len() {
            return Some(vec![]);
        }

        // Tail is beyond size of file
        if tail > data.len() {
            tail = data.len();
        }

        return Some(data[head..tail].to_vec());
    }

    /// Lookup child of parent by name
    pub fn lookup(
        &mut self,
        parent_inode: u32,
        name: &str,
    ) -> Option<&FatDirectoryEntryContainer> {
        let name = name.to_lowercase();

        // If not cached, parse parent first
        if self.dir_cache.get(&parent_inode).is_none() {
            self.list_directory(parent_inode);
        }

        match self.dir_cache.get(&parent_inode) {
            None => None,
            Some(dir) => {
                // Look for child in parent directory
                for child in dir {
                    if child.get_name().to_lowercase() == name {
                        return Some(child);
                    }
                }
                return None;
            }
        }
    }

    /// Get information about given inode
    pub fn get_inode(&self, inode: u32) -> Option<&FatDirectoryEntryContainer> {
        match self.inode_cache.get(&inode) {
            None => None,
            Some(parent_inode) => {
                for child in self.dir_cache.get(&parent_inode).unwrap() {
                    if child.cluster_number() == inode {
                        return Some(&child);
                    }
                }
                return None;
            }
        }
    }

    /// List directory
    pub fn list_directory(
        &mut self,
        inode: u32,
    ) -> Option<&Vec<FatDirectoryEntryContainer>> {
        return get_dir(self, inode);
    }

    /// Get OEM name
    pub fn oem_name(&self) -> &str {
        return str::from_utf8(&self.bs.oem_name).unwrap();
    }

    /// Get FAT type
    pub fn fat_type(&self) -> String {
        return format!("{}", self.fat_type);
    }

    /// Is FAT32
    pub fn is_fat32(&self) -> bool {
        return self.fat_type == FatType::Fat32;
    }
}
