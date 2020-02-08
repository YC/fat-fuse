use byteorder::{LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};

use super::{
    first_sector_of_cluster, read_sector, root_dir_sectors, Fat, Fat32Ebpb,
    FatBpb, FatBs, FatEbpb, FatType,
    FatType::{Fat12, Fat16, Fat32},
};

// Reads reserved and inits Fat struct
pub fn read_reserved<'a>(mut f: File) -> Fat {
    let mut buffer: [u8; 512] = [0; 512];
    f.read_exact(&mut buffer).expect("Cannot read boot sector");

    // Verify signature
    if buffer[510] != 0x55 || buffer[511] != 0xAA {
        // Try to seek for sector 6 - backup boot sector
        f.seek(SeekFrom::Start(512 * 6))
            .expect("Boot sector corrupt, seek failed for backup boot sector");
        f.read_exact(&mut buffer)
            .expect("Boot sector corrupt, cannot read backup boot sector");

        // Verify signature
        if buffer[510] != 0x55 || buffer[511] != 0xAA {
            panic!("Boot and backup boot sector do not have valid signature");
        }
    }

    // Read boot sector
    let bs = FatBs::new(&buffer);
    // Read bpb
    let bpb = FatBpb::new(&buffer);

    // Declare
    let mut fat: Fat = Fat {
        image: f,
        bs,
        bpb,
        ebpb16: None,
        ebpb32: None,
        fat_type: Fat32,
        fat: HashMap::new(),
        dir_cache: HashMap::new(),
        inode_cache: HashMap::new(),
    };

    // Read ebpb
    if fat.bpb.fat_size_16 == 0
        && fat.bpb.total_sectors_16 == 0
        && fat.bpb.total_sectors_32 != 0
    {
        // FAT 32
        fat.ebpb32 = Some(Fat32Ebpb::new(&buffer));
    } else {
        // FAT 12/16
        fat.ebpb16 = Some(FatEbpb::new(&buffer));
    }

    // Ensure that total sectors is not larger than disk size
    let file_size = fat.image.metadata().unwrap().len();
    match fat.bpb.total_sectors_16 {
        0 => {
            assert!(
                file_size
                    >= fat.bpb.total_sectors_32 as u64
                        * fat.bpb.bytes_per_sector as u64,
                "Total sectors larger than disk size"
            );
        }
        _ => {
            assert!(
                file_size
                    >= fat.bpb.total_sectors_16 as u64
                        * fat.bpb.bytes_per_sector as u64,
                "Total sectors larger than disk size"
            );
        }
    }

    // Set type and cluster count
    let fat_type = determine_fat_type(&fat);
    fat.fat_type = fat_type.1;

    // Read FAT
    assert!(fat.bpb.num_fats >= 2);
    // Read all reserved sectors
    // First data sector is cluster 2
    for i in 0..first_sector_of_cluster(&mut fat, 2) {
        // Read into buffer and push to array
        let sector = read_sector(&mut fat, i as u32);
        fat.fat.insert(i as u32, sector);
    }

    return fat;
}

impl FatBs {
    // Reads boot sector
    fn new(boot_record: &[u8; 512]) -> FatBs {
        let mut jump: [u8; 3] = Default::default();
        jump.copy_from_slice(&boot_record[0..3]);
        let mut oem_name: [u8; 8] = Default::default();
        oem_name.copy_from_slice(&boot_record[3..11]);
        return FatBs { jump, oem_name };
    }
}

impl FatBpb {
    // Read Bpb
    fn new(boot_record: &[u8; 512]) -> FatBpb {
        // Create reader
        let mut cursor = Cursor::new(&boot_record[11..]);

        let bytes_per_sector: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let sectors_per_cluster: u8 = cursor.read_u8().unwrap();
        let reserved_clusters: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let num_fats: u8 = cursor.read_u8().unwrap();
        let root_entry_count: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let total_sectors_16: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let media_descriptor: u8 = cursor.read_u8().unwrap();
        let fat_size_16: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let sectors_per_track: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let heads: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let hidden_sectors_count: u32 =
            cursor.read_u32::<LittleEndian>().unwrap();
        let total_sectors_32: u32 = cursor.read_u32::<LittleEndian>().unwrap();
        return FatBpb {
            bytes_per_sector,
            sectors_per_cluster,
            reserved_clusters,
            num_fats,
            root_entry_count,
            total_sectors_16,
            media_descriptor,
            fat_size_16,
            sectors_per_track,
            heads,
            hidden_sectors_count,
            total_sectors_32,
        };
    }
}

impl FatEbpb {
    // Read EBPB
    fn new(boot_record: &[u8; 512]) -> FatEbpb {
        // Create reader (from 0x24)
        let mut cursor = Cursor::new(&boot_record[36..]);

        let drive_number: u8 = cursor.read_u8().unwrap();
        let reserved: u8 = cursor.read_u8().unwrap();
        let boot_signature: u8 = cursor.read_u8().unwrap();
        let mut volume_id: [u8; 4] = Default::default();
        cursor.read_exact(&mut volume_id).unwrap();
        let mut volume_label: [u8; 11] = Default::default();
        cursor.read_exact(&mut volume_label).unwrap();
        let mut fs_type: [u8; 8] = Default::default();
        cursor.read_exact(&mut fs_type).unwrap();

        return FatEbpb {
            drive_number,
            reserved,
            boot_signature,
            volume_id,
            volume_label,
            fs_type,
        };
    }
}

// Read FAT32 EBPB
impl Fat32Ebpb {
    fn new(boot_record: &[u8; 512]) -> Fat32Ebpb {
        // Create reader (from 0x24)
        let mut cursor = Cursor::new(&boot_record[36..]);

        let fat_size_32: u32 = cursor.read_u32::<LittleEndian>().unwrap();
        let flags: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let version: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let root_cluster: u32 = cursor.read_u32::<LittleEndian>().unwrap();
        let fsinfo_sector: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let backup_sector: u16 = cursor.read_u16::<LittleEndian>().unwrap();
        let mut reserved: [u8; 12] = Default::default();
        cursor.read_exact(&mut reserved).unwrap();
        let drive_number: u8 = cursor.read_u8().unwrap();
        let reserved_flags: u8 = cursor.read_u8().unwrap();
        let signature: u8 = cursor.read_u8().unwrap();
        let mut volume_id: [u8; 4] = Default::default();
        cursor.read_exact(&mut volume_id).unwrap();
        let mut volume_label: [u8; 11] = Default::default();
        cursor.read_exact(&mut volume_label).unwrap();
        let mut fs_type: [u8; 8] = Default::default();
        cursor.read_exact(&mut fs_type).unwrap();

        return Fat32Ebpb {
            fat_size_32,
            flags,
            version,
            root_cluster,
            fsinfo_sector,
            backup_sector,
            reserved,
            drive_number,
            reserved_flags,
            signature,
            volume_id,
            volume_label,
            fs_type,
        };
    }
}

// Determines FAT type
fn determine_fat_type(fat: &Fat) -> (u32, FatType) {
    // Find count of sectors occupied by root directory
    let root_dir_sectors = root_dir_sectors(fat);

    // Find FAT size
    let fat_size: u32;
    if fat.bpb.fat_size_16 != 0 {
        fat_size = fat.bpb.fat_size_16.into();
    } else {
        fat_size = fat.ebpb32.as_ref().unwrap().fat_size_32;
    }

    // Find total number of sectors
    let total_sectors: u32;
    if fat.bpb.total_sectors_16 != 0 {
        total_sectors = fat.bpb.total_sectors_16.into();
    } else {
        total_sectors = fat.bpb.total_sectors_32;
    }

    // Find count of sectors in data region
    let data_sectors: u32 = total_sectors
        - (fat.bpb.reserved_clusters as u32
            + (fat.bpb.num_fats as u32 * fat_size)
            + root_dir_sectors as u32);

    // Determine count of clusters
    let cluster_count: u32 = data_sectors / fat.bpb.sectors_per_cluster as u32;

    // Determine type
    if cluster_count < 4085 {
        (cluster_count, Fat12)
    } else if cluster_count < 65525 {
        (cluster_count, Fat16)
    } else {
        (cluster_count, Fat32)
    }
}
