use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::exit;

use cdrom::Disc;
use clap::Parser;
use cue::cd::CD;
use cue::track::Track;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = "Generate CCD and SUB files from BIN/CUE"
)]
struct Args {
    filename: String,
}

fn has_multiple_files(tracks: Vec<Track>) -> bool {
    let mut tracks_iter = tracks.iter();
    let base_file = tracks_iter.next().unwrap().get_filename();
    for track in tracks_iter {
        if track.get_filename() != base_file {
            return true;
        }
    }

    false
}

// TODO handle incorrect sector sizes and remainders
fn sector_count(size: u64, sector_size: u64) -> u64 {
    size / sector_size
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let root = Path::new(&args.filename).parent().unwrap();
    let cue_sheet = std::fs::read_to_string(&args.filename)?;

    let cd = CD::parse(cue_sheet)?;

    let tracks = cd.tracks();
    if has_multiple_files(tracks) {
        println!("This tool currently only supports single-file BIN/CUE images.");
        exit(1);
    }
    let fname = cd.tracks().first().unwrap().get_filename();
    let file = root.join(fname);
    if !file.is_file() {
        println!("Cuesheet file {} does not exist", file.to_string_lossy());
        exit(1);
    }
    let filesize = file.metadata().unwrap().len();
    // TODO deal with non-2352 byte per sector images (treat as an error?)
    let sectors = sector_count(filesize, 2352);
    println!("Image is {} sectors long", sectors);

    let sub_target = file.with_extension("sub");
    let mut sub_write = File::create(sub_target)?;

    let disc = Disc::from_cuesheet(cd, sectors as i64);
    for sector in disc.sectors() {
        sub_write.write_all(&sector.generate_subchannel())?;
    }

    Ok(())
}
