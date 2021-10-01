extern crate clap;
use clap::{App, Arg};

mod fat_fuse;
use fat_fuse::FatFS;

fn main() {
    let matches = App::new("fat-fuse")
        .version("0.1.0")
        .about("Readonly FUSE implemention of FAT12/16/32 filesystems")
        .arg(Arg::with_name("image_file").required(true))
        .arg(Arg::with_name("mount_point").required(true))
        .get_matches();
    let filename = matches.value_of("image_file").unwrap();
    let mount_point = matches.value_of("mount_point").unwrap();

    // Init and mount
    let fat_fs = FatFS::new(filename);
    fuse::mount(fat_fs, &mount_point, &[]).unwrap();
}
