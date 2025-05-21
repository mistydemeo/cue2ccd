use std::fs::File;
use std::io::Write;
use std::path::Path;

use cdrom::Disc;
use clap::Parser;
use cue::cd::CD;
use cue::track::{Track, TrackMode};
use miette::{Diagnostic, Result};
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
enum Cue2CCDError {
    #[error("Couldn't find one or more files specified in the cuesheet.")]
    #[diagnostic(help("Missing files: {}", missing_files.join(", ")))]
    MissingFilesError { missing_files: Vec<String> },

    #[error("Unable to determine the directory {filename} is in!")]
    NoParentError { filename: String },

    #[error("Unable to determine the filename portion of {filename}!")]
    NoFilenameError { filename: String },

    #[error("This tool only supports raw disc images")]
    #[diagnostic(help("cuesheets containing .wav files are not compatible."))]
    WaveFile {},

    #[error("This tool only supports raw disc images")]
    #[diagnostic(help("cuesheets containing ISOs or other non-raw data are not compatible."))]
    CookedData {},

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

/// Fetches unique tracks from the list of tracks.
/// If the same track appears multiple times in a row,
/// returns only a single copy.
fn get_unique_tracks(tracks: &[Track]) -> Vec<String> {
    let mut files = vec![];

    for track in tracks.iter() {
        let filename = track.get_filename();
        if files.last() == Some(&filename) {
            continue;
        }
        files.push(filename);
    }

    files
}

fn main() -> Result<(), miette::Report> {
    work()?;
    Ok(())
}

fn work() -> Result<(), Cue2CCDError> {
    let args = Args::parse();

    let Some(root) = Path::new(&args.filename).parent() else {
        return Err(Cue2CCDError::NoParentError {
            filename: args.filename,
        });
    };
    let Some(basename) = Path::new(&args.filename).file_name() else {
        return Err(Cue2CCDError::NoFilenameError {
            filename: args.filename,
        });
    };
    let path;
    let output_path;
    if let Some(p) = args.output_path {
        path = p;
        output_path = Path::new(&path);
    } else {
        output_path = root;
    }
    // Provides a pattern to build output filenames from
    let output_stem = output_path.join(basename);

    let cue_sheet = std::fs::read_to_string(&args.filename)?;
    // Trim trailing newlines to work around a libcue bug:
    // https://github.com/lipnitsk/libcue/issues/52
    let cue_sheet = cue_sheet.trim_end_matches(['\r', '\n']).to_owned();

    let cd = CD::parse(cue_sheet)?;

    let tracks = cd.tracks();

    // We validate that the track modes are compatible. BIN/CUE can be
    // a variety of different formats, including WAVE files and "cooked"
    // tracks with no error correction metadata. We need all raw files in
    // order to be able to merge into a CloneCD image.
    // In the future, it may be nice to support actually converting tracks
    // into the supported format, but right now that's out of scope.
    validate_mode(&tracks)?;

    let files = get_unique_tracks(&tracks);
    let missing_files = files
        .iter()
        .filter(|f| !root.join(f).is_file())
        .cloned()
        .collect::<Vec<String>>();
    if !missing_files.is_empty() {
        return Err(Cue2CCDError::MissingFilesError { missing_files });
    }

    let sub_target = output_stem.with_extension("sub");
    let mut sub_write = File::create(sub_target)?;

    let disc = Disc::from_cuesheet(cd, root);
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
        } else {
            let mut out_file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&img_target)?;
            for fname in files {
                let mut in_file = File::open(root.join(&fname))?;
                std::io::copy(&mut in_file, &mut out_file)?;
                out_file.flush()?;
            }
        }
    }

    eprintln!(
        "Conversion complete! Created {}",
        output_stem.with_extension("ccd").display()
    );

    Ok(())
}
