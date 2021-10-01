use std::convert::TryInto;
use std::ffi::OsStr;

extern crate lib_fat;
use lib_fat::{Fat, FatDirectoryEntryContainer, FatFileType};

extern crate libc;
use libc::ENOENT;
extern crate time;
use time::Timespec;
extern crate chrono;
use chrono::NaiveDate;

use fuse::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEntry, Request,
};

pub struct FatFS {
    fat: Fat,
}

const TTL: Timespec = Timespec { sec: 1, nsec: 0 };

impl FatFS {
    pub fn new(filename: &str) -> FatFS {
        let fat = Fat::mount_volume(&filename);
        println!("Volume type: {}", fat.fat_type());
        FatFS { fat }
    }
}

impl Filesystem for FatFS {
    /// Read data of specified ino
    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        reply: ReplyData,
    ) {
        match self.fat.get_data(
            ino.try_into().unwrap(),
            offset.try_into().unwrap(),
            size,
        ) {
            Some(data) => {
                reply.data(&data);
            }
            None => {
                reply.error(ENOENT);
            }
        }
    }

    /// Lookup child of parent inode by name
    fn lookup(
        &mut self,
        _req: &Request,
        parent: u64,
        name: &OsStr,
        reply: ReplyEntry,
    ) {
        // If parent inode is 1, corresponds to FAT12/16 cluster number of 0
        let parent_inode = match parent {
            1 => {
                if self.fat.is_fat32() {
                    self.fat.get_root_cluster_number()
                } else {
                    0
                }
            }
            _ => parent.try_into().unwrap(),
        };

        let entry = self.fat.lookup(parent_inode, name.to_str().unwrap());
        match entry {
            None => reply.error(ENOENT),
            Some(entry) => reply.entry(&TTL, &attr(&entry), 0),
        }
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match ino {
            // Root directory
            1 => {
                let attr = FileAttr {
                    ino: 1,
                    size: 0,
                    blocks: 1,
                    atime: Timespec::new(0, 0),
                    mtime: Timespec::new(0, 0),
                    ctime: Timespec::new(0, 0),
                    crtime: Timespec::new(0, 0),
                    kind: FileType::Directory,
                    perm: 0o755,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                    flags: 0,
                };
                reply.attr(&TTL, &attr);
            }
            _ => {
                // File or subdirectory
                let entry = self.fat.get_inode(ino.try_into().unwrap());
                match entry {
                    None => reply.error(ENOENT),
                    Some(entry) => reply.attr(&TTL, &attr(&entry)),
                }
            }
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        // Get root inode number
        let root_inode = self.fat.get_root_cluster_number();

        // Get directory entries
        let dir_option = match ino {
            1 => self.fat.list_directory(root_inode),
            _ => self.fat.list_directory(ino.try_into().unwrap()),
        };
        let dir = match dir_option {
            None => return reply.error(ENOENT),
            Some(dir) => dir,
        };

        // Push . and .. for root
        let mut entries: std::vec::Vec<(u64, fuse::FileType, String)> = vec![];
        if ino == 1 {
            entries.push((1, FileType::Directory, ".".to_string()));
            entries.push((1, FileType::Directory, "..".to_string()));
        }

        // Add entries
        for entry in dir {
            if entry.attribute() & FatFileType::AttrHidden as u8 != 0 {
                continue;
            } else if entry.attribute() & FatFileType::AttrVolumeId as u8 != 0 {
                continue;
            }

            // Only process file and directories
            let inode = entry.cluster_number().into();
            let inode = if inode == root_inode.into() || inode == 0 as u64 {
                1
            } else {
                inode
            };
            let entry_name = entry.get_name();
            if entry.attribute() & FatFileType::AttrDirectory as u8 != 0 {
                entries.push((inode, FileType::Directory, entry_name.clone()));
            } else if entry.attribute() & FatFileType::AttrArchive as u8 != 0 {
                entries.push((
                    inode,
                    FileType::RegularFile,
                    entry_name.clone(),
                ));
            }
        }

        // From lib example
        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize)
        {
            // i + 1 means the index of the next entry
            reply.add(entry.0, (i + 1) as i64, entry.1, entry.2);
        }
        reply.ok();
    }
}

/// Converts directory entry to FileAttr
fn attr(entry: &FatDirectoryEntryContainer) -> FileAttr {
    let kind;
    if entry.attribute() & FatFileType::AttrDirectory as u8 != 0 {
        kind = FileType::Directory;
    } else if entry.attribute() & FatFileType::AttrArchive as u8 != 0 {
        kind = FileType::RegularFile;
    } else {
        panic!("Unrecognized file type");
    }

    return FileAttr {
        ino: entry.cluster_number().try_into().unwrap(),
        size: entry.size() as u64,
        blocks: entry.cluster_count().into(),
        atime: Timespec::new(parse_access_date(&entry), 0),
        mtime: Timespec::new(parse_modify_time(&entry), 0),
        ctime: Timespec::new(parse_create_time(&entry), 0),
        crtime: Timespec::new(parse_create_time(&entry), 0),
        kind: kind,
        perm: 0o755,
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: 0,
        flags: 0,
    };
}

// Parse modify time into timestamp
fn parse_modify_time(entry: &FatDirectoryEntryContainer) -> i64 {
    let (year, month, day, hour, minute, second) = entry.get_write_time();
    if month == 0 || day == 0 {
        return 0;
    }
    return NaiveDate::from_ymd(year.into(), month.into(), day.into())
        .and_hms(hour.into(), minute.into(), second.into())
        .timestamp();
}

// Parse create time into timestamp
fn parse_create_time(entry: &FatDirectoryEntryContainer) -> i64 {
    let (year, month, day, hour, minute, second) = entry.get_creation_time();
    if month == 0 || day == 0 {
        return 0;
    }
    return NaiveDate::from_ymd(year.into(), month.into(), day.into())
        .and_hms(hour.into(), minute.into(), second.into())
        .timestamp();
}

// Parse last access time into timestamp
fn parse_access_date(entry: &FatDirectoryEntryContainer) -> i64 {
    let (year, month, day) = entry.get_last_accessed_date();
    if month == 0 || day == 0 {
        return 0;
    }
    return NaiveDate::from_ymd(year.into(), month.into(), day.into())
        .and_hms(0, 0, 0)
        .timestamp();
}
