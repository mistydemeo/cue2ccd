use std::ffi::OsStr;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use cdrom::cue::cd::CD;
use cdrom::cue::track::{Track, TrackMode};
use cdrom::Disc;
use cdrom::DiscProtection;
use clap::Parser;
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
    // TODO: list choices on this error, also in other places
    #[error("Protection flag provided with invalid protection type!")]
    InvalidProtectionError {},

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
    #[arg(long)]
    pub protection_type: Option<String>,
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

    let cd = CD::parse(cue_sheet)?;

    let tracks = cd.tracks();
    let chosen_protection_type: Option<DiscProtection> = match args
        .protection_type
        .map(|t| t.to_ascii_lowercase())
        .as_deref()
    {
        Some("discguard") => Some(DiscProtection::DiscGuard),
        Some("securom") => Some(DiscProtection::SecuROM),
        Some("libcrypt") => Some(DiscProtection::LibCrypt),
        None => None,
        _ => return Err(Cue2CCDError::InvalidProtectionError {}),
    };

    // Technically speaking, there's no reason you *shouldn't* be able to provide an SBI/LSD
    // file even if you didn't choose protection

    // SBI File Format:
    // Starts with header 0x53 0x42 0x49 0x00 ('S' 'B' 'I' '0x00')
    // The entire rest of the file consists of subQ data, specifically consisting of the actual
    // MSF current subQ was read from, followed by a dummy 0x01 byte, followed by the first 10 bytes
    // of that subQ (so, everything but the CRC16) The exclusion of the CRC16 is obviously
    // annoying, *especially* for SecuROM and LibCrypt. LSD is a better file format, but at the
    // moment, redump will only generate LSD files for PS1 discs, and we do not have the power to
    // change the website; so, until a successor website exists, SBI support is necessary. It's
    // also still preferred by a lot of people and emulators for PS1 for some reason, despite
    // being worse than LSD.

    let mut sbi_lba_array: Vec<i32> = Vec::new();
    let mut sbi_data: Vec<Vec<u8>> = Vec::new();
    // SBI files have never been defined in the cuesheet, and programs (mainly just PS1
    // emulators so far) that make use of them simply check if there's an SBI file with the
    // same basename next to the .cue. If one exists, they use it, otherwise they don't.
    // It seems  best to keep in line with this behavior

    // TODO: is this extension check case sensitive?
    if Path::new(&output_stem.with_extension("sbi")).exists() {
        // SBI files are very small, so it seems best to read the whole thing in first?
        let raw_sbi_data = std::fs::read(Path::new(&output_stem.with_extension("sbi")))?; // Already confirmed it was something, so, this should be fine?
        let (header, data) = raw_sbi_data.split_at(4);

        if header != [83, 66, 73, 00] {
            // Checks for required [S][B][I][0x00] header
            println!("not equal"); // Not sure what to do if for some reason not there
        }
        // should always be multiple of 14
        for (chunkIndex, chunk) in data.chunks(14).enumerate() {
            let mut q = vec![0; 10];
            let mut lba: i32 = 0;
            for (byteIndex, &item) in chunk.iter().enumerate() {
                if byteIndex < 3 {
                    q[byteIndex] = item;
                    // Convert MSF to LBA.
                    if byteIndex == 0 {
                        lba = lba + (4500 * (item as i32));
                    } else if byteIndex == 1 {
                        lba = lba + (60 * (item as i32));
                    } else if byteIndex == 2 {
                        lba = lba + (item as i32);
                    }
                } else if byteIndex > 3 {
                    // Index 3 excluded to ignore dummy 0x01 byte
                    q[byteIndex - 4] = item;
                }
            }
            sbi_lba_array.push(lba);
            sbi_data.push(q);
        }
    }

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
        sub_write.write_all(&sector.generate_subchannel(&chosen_protection_type))?;
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
