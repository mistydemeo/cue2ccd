use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::exit;

use cdrom::{Disc, TrackMode};
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

fn lba_to_msf(lba: i64) -> (i64, i64, i64) {
    (lba / 4500, (lba / 75) % 60, lba % 75)
}

fn write_track(
    writer: &mut File,
    entry: usize,
    pointer: u8,
    track: &cdrom::Track,
) -> io::Result<()> {
    write!(writer, "[Entry {}]\n", entry)?;
    write!(writer, "Session=1\n")?;
    // Pointer is either a track number from 1 to 99, *or* it's a control
    // code. Valid control codes according to the spec are:
    // A0 - P-MIN field indicates the first information track, and P-SEC/P-FRAC are zero
    // A1 - P-MIN field indicates the last information track, and P-SEC/P-FRAC are zero
    // A2 - P-MIN field indicates the start of the leadout, and P-SEC/P-FRAC are zero
    // For more detail, see section 22.3.4.2 of ECMA-130.
    write!(writer, "Point=0x{:02x}\n", pointer)?;

    // Next, based on that value, we need to determine how to set M/S/F.
    // They might not actually be the real timekeeping info, based on the above.
    let m;
    let s;
    let f;
    match pointer {
        0xA0 | 0xA1 => {
            m = track.number as i64;
            s = 0;
            f = 0;
        }
        0xA2 => (m, s, f) = lba_to_msf(track.start + track.length + 150),
        _ => (m, s, f) = lba_to_msf(track.start),
    }

    write!(writer, "ADR=0x01\n")?;
    // Control field. This is a 4-bit value defining the track type.
    // There are more settings, but we only set these two.
    // See section 22.3.1 of ECMA-130.
    let control = if let TrackMode::Audio = track.mode {
        // Audio track, all bits 0
        0
    } else {
        // Data with copy flag set - 0100
        4
    };
    write!(writer, "Control=0x{:02x}\n", control)?;
    // Yes, this is hardcodable despite what it looks like
    write!(writer, "TrackNo=0\n")?;
    // Despite the A-MIN/SEC/FRAC values in the subchannel always containing
    // an absolute timestamp, here they're always zeroed out.
    write!(writer, "AMin=0\n")?;
    write!(writer, "ASec=0\n")?;
    write!(writer, "AFrame=0\n")?;
    // Should probably be calculated based on the pregap
    write!(writer, "ALBA=-150\n")?;
    write!(writer, "Zero=0\n")?;
    // These three next values are the absolute MIN/SEC/FRAC
    write!(writer, "PMin={}\n", m)?;
    write!(writer, "PSec={}\n", s)?;
    write!(writer, "PFrame={}\n", f)?;
    write!(writer, "PLBA={}\n\n", track.start)?;

    Ok(())
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
    let filesize = file.metadata()?.len();
    // TODO deal with non-2352 byte per sector images (treat as an error?)
    let sectors = sector_count(filesize, 2352);
    println!("Image is {} sectors long", sectors);

    let sub_target = file.with_extension("sub");
    let mut sub_write = File::create(sub_target)?;

    let disc = Disc::from_cuesheet(cd, sectors as i64);
    for sector in disc.sectors() {
        sub_write.write_all(&sector.generate_subchannel())?;
    }

    let ccd_target = file.with_extension("ccd");
    let mut ccd_write = File::create(ccd_target)?;

    // Instead of using a real INI parser, write out via format strings.
    // The stuff we're doing here is simple enough.
    // Note that many values here are hardcoded, because we're not doing a
    // full implementation of every CD feature, even if they were in the
    // source BIN/CUE.
    write!(&mut ccd_write, "[CloneCD]\n")?;
    write!(&mut ccd_write, "Version=3\n\n")?;

    write!(&mut ccd_write, "[Disc]\n")?;
    // We always write out exactly 3 TOC entries more than the number of tracks.
    // That accounts for extra TOC entries such as the leadout.
    write!(&mut ccd_write, "TocEntries={}\n", disc.tracks.len() + 3)?;
    // Multisession cuesheets are rare, we're pretending they don't exist
    write!(&mut ccd_write, "Sessions=1\n")?;
    write!(&mut ccd_write, "DataTracksScrambled=0\n")?;
    // CD-TEXT not yet supported
    write!(&mut ccd_write, "CDTextLength=0\n\n")?;

    write!(&mut ccd_write, "[Session 1]\n")?;
    write!(&mut ccd_write, "PreGapMode=2\n")?;
    write!(&mut ccd_write, "PreGapSubC=0\n\n")?;

    // To match other tools, we write track 1 and the final track before
    // going back to write the other tracks.
    let first_track = &disc.tracks[0];
    let last_track = if disc.tracks.len() > 1 {
        &disc.tracks[disc.tracks.len()]
    } else {
        first_track
    };

    let mut entry = 0;

    write_track(&mut ccd_write, entry, 0xA0, first_track)?;
    entry += 1;
    write_track(&mut ccd_write, entry, 0xA1, last_track)?;
    entry += 1;
    write_track(&mut ccd_write, entry, 0xA2, last_track)?;
    entry += 1;

    for track in disc.tracks {
        write_track(&mut ccd_write, entry, track.number as u8, &track)?;
        entry += 1;
    }

    Ok(())
}
