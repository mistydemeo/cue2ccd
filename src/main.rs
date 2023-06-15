use std::env::args;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::exit;

use cdrom_crc::{crc16, CRC16_INITIAL_CRC};
use cue::cd::{DiscMode, CD};
use cue::track::{Track, TrackMode};

struct Disc {
    tracks: Vec<DiscTrack>,
    sector_count: i64,
}

impl Disc {
    fn sectors(&self) -> SectorIterator {
        SectorIterator {
            current: 0,
            disc: self,
        }
    }
}

struct SectorIterator<'a> {
    current: i64,
    disc: &'a Disc,
}

impl<'a> SectorIterator<'a> {
    fn sector_from_number(&self, sector: i64) -> Option<Sector> {
        // We should start at or around sector 0 (actually 150, but who's counting)
        // (me, I am), which means we can iterate through tracks and indices in order
        // safely until we hit the one that starts at our sector.
        for track in &self.disc.tracks {
            for (i, index) in track.indices.iter().enumerate() {
                // Edge of the index is either the start of the next index (if there's
                // another track) or the end of the track.
                let boundary = if let Some(next) = track.indices.get(i + 1) {
                    next.start as i64
                } else {
                    track.start + track.length
                };

                if index.start as i64 <= sector && boundary >= sector {
                    // Pregap counts backwards to the start of the following
                    // index. Yes, really!
                    let relative_position = if index.number == 0 {
                        index.end - sector
                    } else {
                        sector - index.start as i64
                    };

                    return Some(Sector {
                        start: sector,
                        relative_position,
                        size: 2352, // TODO un-hardcode this
                    });
                }
            }
        }

        None
    }
}

impl<'a> Iterator for SectorIterator<'a> {
    type Item = Sector;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.disc.sector_count {
            return None;
        }

        let sector = self.sector_from_number(self.current);

        self.current += 1;

        sector
    }
}

impl Disc {
    fn from_cuesheet(cuesheet: CD, sector_count: i64) -> Disc {
        let mut tracks = vec![];
        for (i, track) in cuesheet.tracks().iter().enumerate() {
            let tracknum = i + 1;

            let start = track.get_start();
            // The last track on the disc will have indeterminate length,
            // because the cuesheet doesn't store that; we need to calculate
            // it from the size of the disc.
            let length = track.get_length().unwrap_or(sector_count - start);

            let mut indices = vec![];
            for i in 0..99 {
                if let Some(index) = track.get_index(i) {
                    // Cuesheet doesn't actually track the end of an index,
                    // so we need to either calculate the boundary of the next
                    // index within the track or the end of the track itself.
                    let end = if let Some(next) = track.get_index(i + 1) {
                        next as i64 - 1
                    } else {
                        start + track.get_length().unwrap_or(sector_count)
                    };

                    indices.push(Index {
                        number: i as usize,
                        start: index,
                        end,
                    });
                }
            }

            tracks.push(DiscTrack {
                number: tracknum,
                start: track.get_start(),
                length,
                indices,
            });
        }

        Disc {
            tracks,
            sector_count,
        }
    }
}

struct DiscTrack {
    number: usize,
    start: i64,
    length: i64,
    indices: Vec<Index>,
}

struct Index {
    number: usize,
    start: isize,
    end: i64,
}

#[derive(Debug)]
struct Sector {
    start: i64,
    relative_position: i64,
    size: usize,
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

// The subchannel data contains extra sidecar metadata required to read
// the disc, but which isn't a part of the data itself.
// Some applications can read CloneCD data with zeroed out subchannel data
// provided that the more verbose form of the CCD control file is used,
// but other applications require real subchannel data no matter what.
//
// The CloneCD format stores the subchannel data in a sidecar file, which
// is essentially identical to the data on the disc with a few exceptions:
// 1) The leadin (the first 150 sectors) is omitted, and so is the
//    table of contents that's usually stored there.
// 2) The first two subchannel bytes, which contain sync words, are omitted.
// 3) The subchannel data is unrolled into eight sequential sections of
//    12 bytes instead of interleaved bits. This is easier to read and write
//    in non-streaming applications.
// In total, we need to write 96 bytes of subchannel data for each sector.
//
// More information is in ECMA-130:
// http://www.ecma-international.org/publications/standards/Ecma-130.htm

// TODO handle incorrect sector sizes and remainders
fn sector_count(size: u64, sector_size: u64) -> u64 {
    size / sector_size
}

fn bcd(dec: i64) -> i64 {
    ((dec / 10) << 4) | (dec % 10)
}

fn generate_q_subchannel(
    absolute_sector: i64,
    relative_sector: i64,
    last_sector: i64,
    track: i64,
    is_pregap: bool,
    track_type: TrackMode,
) -> Vec<u8> {
    // This channel made up of a sequence of bits; we'll start by
    // zeroing it out, then setting individual bits.
    let mut q = vec![0; 12];

    // First four bits are the control field.
    // We only care about setting the data bit, 1; the others are
    // irrelevant for this application.
    match track_type {
        TrackMode::Audio => (),
        _ => q[0] |= 1 << 6,
    };

    // Next four bits indicate the mode of the Q channel.
    // There are three modes:
    // * 1 - Table of contents (used during the lead-in)
    // * 2 - Media Catalog Number
    // * 3 - International Standard Recording Code (ISRC)
    // In practice, we're always generating mode 1
    // every sector so we'll hardcode this.
    q[0] |= 1 << 0;
    // OK, it's data time! This is the next 9 bytes.
    // This contains timing info for the current track.
    if is_pregap {
        // TODO validate the track number going in here
        q[1] = track as u8 + 1;
    } else {
        q[1] = track as u8;
    }

    // Next is the index. While it supports values up to 99,
    // we're only going to use two values:
    // 00 - Pregap or postgap
    // 01 - First index within the track, or leadout
    if is_pregap {
        q[2] = 0;
    } else {
        q[2] = 1;
    }

    // The next three fields, MIN, SEC, and FRAC, are the
    // running time within each index.
    // FRAC is a unit of 1/75th of a second, e.g. the
    // duration of exactly one sector.
    // In the pregap, this starts at the pregap duration
    // and counts down to 0.
    // In the actual content, this starts at 0 and
    // counts up.
    let time;
    if is_pregap {
        time = 150 + 1 - (absolute_sector - last_sector);
    } else {
        time = relative_sector;
    }
    // MIN
    q[3] = bcd(time / 4500) as u8;
    // SEC
    q[4] = bcd((time / 75) % 60) as u8;
    // FRAC
    q[5] = bcd(time % 75) as u8;
    // Next byte is always zero
    q[6] = 0;
    // The next three bytes provide an absolute timestamp,
    // rather than a timestamp within the current track.
    // These three fields, A-MIN, A-SEC, and A-FRAC, are
    // stored the same way as the relative timestamps.
    q[7] = bcd(absolute_sector / 4500) as u8;
    q[8] = bcd((absolute_sector / 75) % 60) as u8;
    q[9] = bcd(absolute_sector % 75) as u8;
    // The last two bytes contain a CRC of the main data.
    let crc = crc16(&q[0..10], CRC16_INITIAL_CRC);
    q[10] = ((crc >> 8) & 0xFF) as u8;
    q[11] = (crc & 0xFF) as u8;

    q
}

fn main() -> io::Result<()> {
    // This is all super ugly obviously but it's just standing in for real
    // arg parsing to come later.
    let mut argv = args();
    argv.next(); // programname
    let cue_sheet;
    let root;
    let fname;
    if let Some(filename) = argv.next() {
        fname = filename;
        root = Path::new(&fname).parent().unwrap();
        cue_sheet = std::fs::read_to_string(&fname)?;
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

    for (i, track) in cd.tracks().iter().enumerate() {
        let track_num = i as i64 + 1;

        // The last track on the disc will have indeterminate length,
        // because the cuesheet doesn't store that; we need to calculate
        // it from the size of the disc.
        let track_length = track
            .get_length()
            .unwrap_or(sectors as i64 - track.get_start());

        println!("Track: {}", track_num);
        println!("Index 0: {}", track.get_index(0).unwrap_or(-1));
        println!("Index 1: {}", track.get_index(1).unwrap_or(-1));
        println!("Pregap: {}", track.get_zero_pre().unwrap_or(-1));
        println!("Postgap: {}", track.get_zero_post().unwrap_or(-1));
        println!("Start: {}; length: {}", track.get_start(), track_length);
        println!();

        let start = track.get_start();

        // Pregap - not every track has one
        if let Some(pregap) = track.get_zero_pre() {
            for lba in (start - pregap)..start {
                // For the pregap, always fill the P data sector with FFs.
                let p: Vec<u8> = vec![0xFF; 12];
                let q = generate_q_subchannel(
                    // First 150 sectors are omitted
                    lba + 151,
                    lba - pregap,
                    start,
                    track_num,
                    true,
                    track.get_mode(),
                );
                assert_eq!(12, q.len());
                // We only write out actual P and Q data;
                // the rest is undefined by the CD-ROM spec, and we're
                // not making CD-TEXT or CD+G discs.
                let rest: Vec<u8> = vec![0; 72];
                sub_write.write_all(&p)?;
                sub_write.write_all(&q)?;
                sub_write.write_all(&rest)?;
            }
        }

        for lba in start..track_length + start {
            // The first sector of the disc, and only the first sector,
            // gets an FFed out P sector like a pregap. Every other non-pregap
            // sector uses 0s.
            // For players which ignore the Q subchannel, this allows
            // locating the start of tracks.
            let p: Vec<u8> = if lba == 0 {
                vec![0xFF; 12]
            } else {
                vec![0; 12]
            };
            assert_eq!(12, p.len());
            let q = generate_q_subchannel(
                // First 150 sectors are omitted
                lba + 151,
                lba - start,
                start + track_length,
                track_num,
                false,
                track.get_mode(),
            );
            assert_eq!(12, q.len());
            let rest: Vec<u8> = vec![0; 72];
            sub_write.write_all(&p)?;
            sub_write.write_all(&q)?;
            sub_write.write_all(&rest)?;
        }
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
