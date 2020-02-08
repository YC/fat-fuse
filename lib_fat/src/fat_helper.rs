use std::io::prelude::*;
use std::io::Read;
use std::io::SeekFrom;

use super::{
    Fat,
    FatType::{Fat12, Fat16, Fat32},
};

/// Reads the specified sector
pub fn read_sector(fat: &mut Fat, sector_number: u32) -> Vec<u8> {
    // Seek
    fat.image
        .seek(SeekFrom::Start(
            fat.bpb.bytes_per_sector as u64 * sector_number as u64,
        ))
        .expect("Cannot seek to cluster");

    // Read
    let mut buffer = vec![0u8; fat.bpb.bytes_per_sector as usize];
    fat.image.read_exact(&mut buffer).expect("Cannot read cluster");
    return buffer;
}

/// Reads the cluster starting with sector
pub fn read_cluster(fat: &mut Fat, first_sector: u32) -> Vec<u8> {
    let mut data = vec![];
    for i in 0..fat.bpb.sectors_per_cluster {
        data.extend(read_sector(fat, first_sector + i as u32));
    }
    return data;
}

/// Determine FAT entry offset -> (sector number, entry offset),
pub fn determine_fat_entry_offset(
    fat: &Fat,
    cluster_number: u32,
) -> (u32, u32) {
    let fat_offset = match fat.fat_type {
        Fat16 => cluster_number * 2,
        Fat32 => cluster_number * 4,
        Fat12 => cluster_number + (cluster_number / 2),
    };

    // Sector number of FAT sector containing entry for cluster
    let fat_sector_number = fat.bpb.reserved_clusters as u32
        + (fat_offset / fat.bpb.bytes_per_sector as u32);
    let fat_entry_offset = fat_offset % fat.bpb.bytes_per_sector as u32;

    // Read FAT sector
    return match fat.fat_type {
        Fat32 => {
            // Read ebpb extension flags
            let flags = fat.ebpb32.as_ref().unwrap().flags;
            if flags & 0b0000000100000000 == 0 {
                // 7th bit not set, FAT is mirrored to all FATs
                (fat_sector_number, fat_entry_offset)
            } else {
                // FAT is not mirrored
                let active_fat = (flags & 0b1111000000000000) >> 12;
                let fat_size: u32 = calculate_fat_size(fat);
                (
                    fat_sector_number + active_fat as u32 * fat_size,
                    fat_entry_offset,
                )
            }
        }
        _ => (fat_sector_number, fat_entry_offset),
    };
}

/// Read FAT entry
pub fn read_fat_entry(
    fat: &Fat,
    cluster_number: u32,
    fat_sector_number: u32,
    fat_entry_offset: u32,
) -> u32 {
    // Read FAT sector
    let sector = fat.fat.get(&fat_sector_number).unwrap();

    match fat.fat_type {
        Fat12 => {
            let split = fat.bpb.bytes_per_sector as u32 - 1;

            let cluster_entry_value: u32 = if fat_entry_offset == split {
                // Entry spans over 2 sectors
                let sector1 = fat.fat.get(&(fat_sector_number + 1)).unwrap();
                (sector[fat_entry_offset as usize] as u32)
                    | (sector1[0] as u32) << 8
            } else {
                // In 1 cluster
                (sector[fat_entry_offset as usize] as u32)
                    | (sector[fat_entry_offset as usize + 1] as u32) << 8
            };

            if cluster_number & 0x0001 != 0 {
                // Odd cluster number
                return cluster_entry_value >> 4;
            } else {
                // Even cluster number
                return cluster_entry_value & 0x0FFF;
            }
        }
        Fat16 => {
            return (sector[fat_entry_offset as usize] as u32)
                | (sector[fat_entry_offset as usize + 1] as u32) << 8
        }
        Fat32 => {
            let cluster_entry_value = (sector[fat_entry_offset as usize + 0]
                as u32)
                | (sector[fat_entry_offset as usize + 1] as u32) << 8
                | (sector[fat_entry_offset as usize + 2] as u32) << 16
                | (sector[fat_entry_offset as usize + 3] as u32) << 24;
            // Higher 4 bits are reserved
            return cluster_entry_value & 0x0FFFFFFF;
        }
    }
}

/// Sectors occupied by root directory
pub fn root_dir_sectors(fat: &Fat) -> u16 {
    // ceil of (number of root entries * 32 bytes per entry) / bytes per sector
    // Note: is 0 on FAT32 volumes
    return ((fat.bpb.root_entry_count * 32) + (fat.bpb.bytes_per_sector - 1))
        / fat.bpb.bytes_per_sector;
}

/// Calculate FAT size
fn calculate_fat_size(fat: &Fat) -> u32 {
    if fat.bpb.fat_size_16 != 0 {
        return fat.bpb.fat_size_16.into();
    } else {
        return fat.ebpb32.as_ref().unwrap().fat_size_32;
    }
}

/// Determine first sector of cluster
pub fn first_sector_of_cluster(fat: &mut Fat, cluster_number: u32) -> u32 {
    // Sectors occupied by root directory
    let root_dir_sectors = root_dir_sectors(fat);

    // Start of data region, first sector of cluster 2
    let fat_size: u32 = calculate_fat_size(fat);
    let first_data_sector = fat.bpb.reserved_clusters as u32
        + (fat.bpb.num_fats as u32 * fat_size)
        + root_dir_sectors as u32;

    return ((cluster_number - 2) * fat.bpb.sectors_per_cluster as u32)
        + first_data_sector;
}

/// Determine number of clusters of file
pub fn file_cluster_count(fat: &Fat, cluster_number: u32) -> u32 {
    // Empty file
    if cluster_number == 0 {
        return 0;
    }

    let mut n_blocks = 0;
    let mut eof = false;
    let mut current_block = cluster_number;

    while !eof {
        // Determine FAT entry location
        let (fat_sector_number, fat_entry_offset) =
            determine_fat_entry_offset(fat, current_block);
        // Lookup FAT entry
        let fat_entry = read_fat_entry(
            fat,
            cluster_number,
            fat_sector_number,
            fat_entry_offset,
        );
        // Is EOF and set next block
        eof = is_eof(fat, fat_entry) || fat_entry == 0;
        current_block = fat_entry;
        n_blocks += 1;
    }

    return n_blocks;
}

/// Whether FAT entry indicate end of file
fn is_eof(fat: &Fat, fat_entry: u32) -> bool {
    return match fat.fat_type {
        Fat12 => fat_entry >= 0x0FF8,
        Fat16 => fat_entry >= 0xFFF8,
        Fat32 => fat_entry >= 0x0FFFFFF8,
    };
}

/// Read data
pub fn read_data(fat: &mut Fat, cluster_number: u32) -> (Vec<u8>, Option<u32>) {
    // Empty file
    if cluster_number == 0 {
        return (vec![], None);
    }

    // Find sector number
    let sector_number = first_sector_of_cluster(fat, cluster_number);
    // Read cluster
    let sector = read_cluster(fat, sector_number);

    // FAT lookup to see there is more
    let (fat_sector_number, fat_entry_offset) =
        determine_fat_entry_offset(fat, cluster_number);
    let fat_entry = read_fat_entry(
        fat,
        cluster_number,
        fat_sector_number,
        fat_entry_offset,
    );

    let is_eof = is_eof(fat, fat_entry) || fat_entry == 0;
    return match is_eof {
        true => (sector, None),
        false => (sector, Some(fat_entry)),
    };
}

/// Read all sectors of file
pub fn read_file_full(fat: &mut Fat, cluster_number: u32) -> Vec<u8> {
    let mut data: Vec<u8> = Vec::new();

    // Read extent
    let (mut sector, mut fat_entry_option) = read_data(fat, cluster_number);

    while let Some(fat_entry) = fat_entry_option {
        // Append sector
        data.append(&mut sector);

        // Read following cluster
        let new_data = read_data(fat, fat_entry);
        sector = new_data.0;
        fat_entry_option = new_data.1;
    }
    // Append last sector
    data.append(&mut sector);

    return data;
}
