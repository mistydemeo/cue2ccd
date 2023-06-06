use std::env::args;
use std::io;
use std::process::exit;

use cue::cd::{DiscMode, CD};
use cue::track::Track;

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

fn main() -> io::Result<()> {
    let mut argv = args();
    argv.next(); // programname
    let cue_sheet;
    if let Some(filename) = argv.next() {
        cue_sheet = std::fs::read_to_string(filename)?;
    } else {
        println!("No cuesheet provided");
        exit(1);
    }

    let cd = CD::parse(cue_sheet)?;

    let tracks = cd.tracks();
    if has_multiple_files(tracks) {
        println!("This tool currently only supports single-file BIN/CUE images.");
        exit(1);
    }

    println!("Number of tracks: {}", cd.get_track_count());
    let mode = match cd.get_mode() {
        DiscMode::CD_DA => "CD-DA",
        DiscMode::CD_ROM => "CD-ROM",
        DiscMode::CD_ROM_XA => "CD-ROM XA",
    };
    println!("Mode: {}", mode);

    Ok(())
}
