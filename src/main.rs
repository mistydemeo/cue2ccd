use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::exit;

use cdrom::Disc;
use clap::Parser;
use cue::cd::CD;
use cue::track::{Track, TrackMode};
use miette::{Diagnostic, Result};
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
enum Cue2CCDError {
    #[error("This tool currently only supports single-file BIN/CUE images.")]
    #[diagnostic(help("Please specify a cuesheet with a single BIN file. You can convert a multi-track disc image into a single track image using chdman or binmerge."))]
    MultipleFilesError {},

    #[error("A data file specified in the cuesheet is missing.")]
    #[diagnostic(help("Missing file: {}", missing_file.display()))]
    MissingFileError { missing_file: std::path::PathBuf },

    #[error("This tool only supports raw disc images")]
    #[diagnostic(help("cuesheets containing .wav files are not compatible."))]
    WaveFile {},

    #[error("This tool only supports raw disc images")]
    #[diagnostic(help("cuesheets containing ISOs or other non-raw data are not compatible."))]
    CookedData {},

    #[error("The provided disc image has an invalid file size")]
    #[diagnostic(help("Check if the .bin for your disc image is corrupted."))]
    InvalidFilesizeError {},

    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    Cue(#[from] std::ffi::NulError),
}

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = "Generate CCD and SUB files from BIN/CUE"
)]
struct Args {
    filename: String,
    #[arg(long, default_value_t = false)]
    skip_img_copy: bool,
    #[arg(long)]
    output_path: Option<String>,
}

fn has_multiple_files(tracks: &[Track]) -> bool {
    let mut tracks_iter = tracks.iter();
    let base_file = tracks_iter.next().unwrap().get_filename();
    for track in tracks_iter {
        if track.get_filename() != base_file {
            return true;
        }
    }

    false
}

fn validate_mode(tracks: &[Track]) -> Result<(), Cue2CCDError> {
    for track in tracks {
        if track.get_filename().ends_with(".wav") {
            return Err(Cue2CCDError::WaveFile {});
        }
        match track.get_mode() {
            TrackMode::Mode1 | TrackMode::Mode2 | TrackMode::Mode2Form1 | TrackMode::Mode2Form2 => {
                return Err(Cue2CCDError::CookedData {});
            }
            _ => (),
        }
    }
    Ok(())
}

fn sector_count(size: u64, sector_size: u64) -> Result<u64, Cue2CCDError> {
    if size % sector_size != 0 {
        return Err(Cue2CCDError::InvalidFilesizeError {});
    }
    Ok(size / sector_size)
}

fn main() -> Result<(), miette::Report> {
    work()?;
    Ok(())
}

fn work() -> Result<(), Cue2CCDError> {
    let args = Args::parse();

    let root = Path::new(&args.filename).parent().unwrap();
    let path;
    let output_path;
    if let Some(p) = args.output_path {
        path = p;
        output_path = Path::new(&path);
    } else {
        output_path = root.clone();
    }
    // Provides a pattern to build output filenames from
    let output_stem = output_path.join(Path::new(&args.filename).file_stem().unwrap());

    let cue_sheet = std::fs::read_to_string(&args.filename)?;

    let cd = CD::parse(cue_sheet)?;

    let tracks = cd.tracks();

    // Reconstructing a new index would be easier if we could produce a new
    // cuesheet, or by refactoring the construction code in the cdrom crate to
    // be a bit less dependent on a cuesheet. This is a nice stretch goal for
    // the future. In the meantime, users can consolidate their multi-track
    // bin/cues using chdman or something else.
    // Note that while we don't actually read the data file ourself, consumers
    // of the CUE/SUB files produced by this tool won't be able to understand
    // split images.
    if has_multiple_files(&tracks) {
        return Err(Cue2CCDError::MultipleFilesError {})?;
    }
    validate_mode(&tracks)?;

    let fname = cd.tracks().first().unwrap().get_filename();
    let file = root.join(fname);
    if !file.is_file() {
        return Err(Cue2CCDError::MissingFileError { missing_file: file })?;
    }
    let filesize = file.metadata()?.len();
    let sectors = sector_count(filesize, 2352)?;
    println!("Image is {} sectors long", sectors);

    let sub_target = output_stem.with_extension("sub");
    let mut sub_write = File::create(sub_target)?;

    let disc = Disc::from_cuesheet(cd, sectors as i64);
    for sector in disc.sectors() {
        sub_write.write_all(&sector.generate_subchannel())?;
    }

    let ccd_target = output_stem.with_extension("ccd");
    let mut ccd_write = File::create(ccd_target)?;
    disc.write_ccd(&mut ccd_write)?;

    if !args.skip_img_copy {
        let img_target = output_stem.with_extension("img");
        if img_target.exists() {
            eprintln!(
                "A .img file at path {} already exists; skipping copy",
                img_target.as_path().display()
            );
        }
    }

    Ok(())
}
