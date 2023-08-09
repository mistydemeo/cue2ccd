use std::fs::File;
use std::io::{self, Write};

use cdrom_crc::{crc16, CRC16_INITIAL_CRC};
use cue::cd::CD;
use cue::track;

fn lba_to_msf(lba: i64) -> (i64, i64, i64) {
    (lba / 4500, (lba / 75) % 60, lba % 75)
}

pub struct Disc {
    pub tracks: Vec<Track>,
    pub sector_count: i64,
}

impl Disc {
    pub fn sectors(&self) -> SectorIterator {
        SectorIterator {
            current: 0,
            disc: self,
        }
    }

    pub fn write_ccd(&self, writer: &mut File) -> io::Result<()> {
        // Instead of using a real INI parser, write out via format strings.
        // The stuff we're doing here is simple enough.
        // Note that many values here are hardcoded, because we're not doing a
        // full implementation of every CD feature, even if they were in the
        // source BIN/CUE.
        writeln!(writer, "[CloneCD]")?;
        write!(writer, "Version=3\n\n")?;

        writeln!(writer, "[Disc]")?;
        // We always write out exactly 3 TOC entries more than the number of tracks.
        // That accounts for extra TOC entries such as the leadout.
        writeln!(writer, "TocEntries={}", self.tracks.len() + 3)?;
        // Multisession cuesheets are rare, we're pretending they don't exist
        writeln!(writer, "Sessions=1")?;
        writeln!(writer, "DataTracksScrambled=0")?;
        // CD-TEXT not yet supported
        write!(writer, "CDTextLength=0\n\n")?;

        // To match other tools, we write track 1 and the final track before
        // going back to write the other tracks.
        let first_track = &self.tracks[0];
        let last_track = if self.tracks.len() > 1 {
            &self.tracks[self.tracks.len() - 1]
        } else {
            first_track
        };

        writeln!(writer, "[Session 1]")?;
        // Appears to be the type of the first track;
        // even in a mixed-mode disc, this is only specified once.
        // Is it possible for this to differ from the type of the first track? Unclear.
        writeln!(writer, "PreGapMode={}", first_track.mode.as_u8())?;
        // Appears to be subchannel for pregap according to Aaru:
        // https://github.com/aaru-dps/Aaru/blob/5410ae5e74f2177887cd1e0e1866d8d55cf244d9/Aaru.Images/CloneCD/Constants.cs#L50
        // Unclear what the "correct" value is, but safe to hardcode.
        write!(writer, "PreGapSubC=0\n\n")?;

        let mut entry = 0;

        self.write_track(writer, entry, Pointer::FirstTrack, first_track)?;
        entry += 1;
        self.write_track(writer, entry, Pointer::LastTrack, last_track)?;
        entry += 1;
        self.write_track(writer, entry, Pointer::LeadOut, last_track)?;
        entry += 1;

        for track in &self.tracks {
            self.write_track(writer, entry, Pointer::Track(track.number), track)?;
            entry += 1;
        }

        // Next, we want to handle writing out the track index.
        // This is a vaguely cuesheet-like format that's optional.
        for track in &self.tracks {
            self.write_track_entry(writer, track)?;
        }

        Ok(())
    }

    fn write_track(
        &self,
        writer: &mut File,
        entry: usize,
        pointer: Pointer,
        track: &Track,
    ) -> io::Result<()> {
        // The data in a CCD file is a low-level representation of the disc's leadin
        // in a plaintext INI format.
        // For some more information keys and their values, see
        // https://psx-spx.consoledev.net/cdromdrive/
        writeln!(writer, "[Entry {}]", entry)?;
        writeln!(writer, "Session=1")?;
        // Pointer is either a track number from 1 to 99, *or* it's a control
        // code. Valid control codes according to the spec are:
        // A0 - P-MIN field indicates the first information track, and P-SEC/P-FRAC are zero
        // A1 - P-MIN field indicates the last information track, and P-SEC/P-FRAC are zero
        // A2 - P-MIN field indicates the start of the leadout, and P-SEC/P-FRAC are zero
        // For more detail, see section 22.3.4.2 of ECMA-130.
        writeln!(writer, "Point=0x{:02x}", pointer.as_u8())?;

        // Next, based on that value, we need to determine how to set M/S/F.
        // They might not actually be the real timekeeping info, based on the above.
        let lba;
        let m;
        let s;
        let f;
        match pointer {
            Pointer::FirstTrack | Pointer::LastTrack => {
                lba = track.number as i64 * 4500 - 150;
                m = track.number as i64;
                s = 0;
                f = 0;
            }
            Pointer::LeadOut => {
                lba = self.sector_count;
                // M/S/F differs from LBA by pregap size
                // Right now we're hardcoding that.
                // Should un-hardcode this later, but this also smells
                // suspiciously like an off-by-150 error on one side?
                (m, s, f) = lba_to_msf(lba + 150);
            }
            _ => {
                lba = track.start;
                (m, s, f) = lba_to_msf(track.start + 150);
            }
        }

        writeln!(writer, "ADR=0x01")?;
        // Control field. This is a 4-bit value defining the track type.
        // There are more settings, but we only set these two.
        // See section 22.3.1 of ECMA-130.
        // TODO: Ensure this control code is correct for leadin and leadout.
        // One real disc had 0 for the leadin when the first track was data,
        // while other discs use 4. 4 is *probably* safe.
        let control = if let TrackMode::Audio = track.mode {
            // Audio track, all bits 0
            0
        } else {
            // Data with copy flag set - 0100
            4
        };
        writeln!(writer, "Control=0x{:02x}", control)?;
        // Yes, this is hardcodable despite what it looks like
        writeln!(writer, "TrackNo=0")?;
        // Despite the A-MIN/SEC/FRAC values in the subchannel always containing
        // an absolute timestamp, here they're always zeroed out.
        writeln!(writer, "AMin=0")?;
        writeln!(writer, "ASec=0")?;
        writeln!(writer, "AFrame=0")?;
        // Should probably be calculated based on the pregap
        writeln!(writer, "ALBA=-150")?;
        writeln!(writer, "Zero=0")?;
        // These three next values are the absolute MIN/SEC/FRAC
        writeln!(writer, "PMin={}", m)?;
        writeln!(writer, "PSec={}", s)?;
        writeln!(writer, "PFrame={}", f)?;
        write!(writer, "PLBA={}\n\n", lba)?;

        Ok(())
    }

    fn write_track_entry(&self, writer: &mut File, track: &Track) -> io::Result<()> {
        writeln!(writer, "[TRACK {}]", track.number)?;
        writeln!(writer, "MODE={}", track.mode.as_u8())?;

        for index in &track.indices {
            writeln!(writer, "INDEX {}={}", index.number, index.start)?;
        }

        Ok(())
    }
}

pub struct SectorIterator<'a> {
    current: i64,
    disc: &'a Disc,
}

impl<'a> SectorIterator<'a> {
    pub fn sector_from_number(&self, sector: i64) -> Option<Sector> {
        // We should start at or around sector 0 (actually 150, but who's counting)
        // (me, I am), which means we can iterate through tracks and indices in order
        // safely until we hit the one that starts at our sector.
        for track in &self.disc.tracks {
            for (i, index) in track.indices.iter().enumerate() {
                // Edge of the index is either the start of the next index (if there's
                // another index) or the end of the track.
                let boundary = if let Some(next) = track.indices.get(i + 1) {
                    next.start
                } else {
                    track.start + track.length
                };

                if index.start <= sector && boundary >= sector {
                    // Yes, it's okay for this to be negative! Pregap counts backwards
                    // to the start of the following index.
                    let relative_position = sector - track.start;

                    return Some(Sector {
                        start: sector,
                        // Convenience for indexing relative to the start of the disc,
                        // rather than the start of the disc image.
                        // Yes, it means the first sector isn't sector 1.
                        absolute_start: sector + 150,
                        relative_position,
                        size: 2352, // TODO un-hardcode this
                        // Worry about lifetimes later, this is small anyway
                        track: track.clone(),
                        index: index.clone(),
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
    pub fn from_cuesheet(cuesheet: CD, sector_count: i64) -> Disc {
        let mut tracks = vec![];
        for (i, track) in cuesheet.tracks().iter().enumerate() {
            let tracknum = i as u8 + 1;

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
                        number: i as u8,
                        start: index as i64,
                        end,
                    });
                }
            }

            tracks.push(Track {
                number: tracknum,
                start: track.get_start(),
                length,
                indices,
                mode: TrackMode::from_cue_mode(track.get_mode()),
            });
        }

        Disc {
            tracks,
            sector_count,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Track {
    pub number: u8,
    pub start: i64,
    pub length: i64,
    pub indices: Vec<Index>,
    pub mode: TrackMode,
}

// Ugly workaround to avoid embedding cue types, rework later
#[derive(Clone, Copy, Debug)]
pub enum TrackMode {
    Audio,
    /// 2048-byte data without ECC
    Mode1,
    /// 2048-byte data with ECC
    Mode1Raw,
    /// 2336-byte data without ECC
    Mode2,
    /// 2048-byte data (CD-ROM XA)
    Mode2Form1,
    /// 2324-byte data (CD-ROM XA)
    Mode2Form2,
    /// 2332-byte data (CD-ROM XA)
    Mode2FormMix,
    /// 2336-byte data with ECC
    Mode2Raw,
}

impl TrackMode {
    fn from_cue_mode(mode: track::TrackMode) -> TrackMode {
        match mode {
            track::TrackMode::Audio => TrackMode::Audio,
            track::TrackMode::Mode1 => TrackMode::Mode1,
            track::TrackMode::Mode1Raw => TrackMode::Mode1Raw,
            track::TrackMode::Mode2 => TrackMode::Mode2,
            track::TrackMode::Mode2Form1 => TrackMode::Mode2Form1,
            track::TrackMode::Mode2Form2 => TrackMode::Mode2Form2,
            track::TrackMode::Mode2FormMix => TrackMode::Mode2FormMix,
            track::TrackMode::Mode2Raw => TrackMode::Mode2Raw,
        }
    }

    pub fn as_u8(&self) -> u8 {
        match self {
            TrackMode::Audio => 0,
            TrackMode::Mode1 | TrackMode::Mode1Raw => 1,
            TrackMode::Mode2
            | TrackMode::Mode2Raw
            | TrackMode::Mode2Form1
            | TrackMode::Mode2Form2
            | TrackMode::Mode2FormMix => 2,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Index {
    // Number of the current index; index 0 is the pregap, index 1 onward are the track proper
    pub number: u8,
    // Start of the current index, in sectors
    pub start: i64,
    // End of the current index, in sectors
    pub end: i64,
}

#[derive(Debug)]
pub struct Sector {
    // Sector number, relative to the start of the image
    pub start: i64,
    // Sector number, relative to the start of the disc
    pub absolute_start: i64,
    // Relative position to index 1 of the current track
    pub relative_position: i64,
    // Size of the sector, in bytes
    pub size: usize,
    // Metadata for the current track
    pub track: Track,
    // Metadata for the current index
    pub index: Index,
}

fn bcd(dec: i64) -> u8 {
    (((dec / 10) << 4) | (dec % 10)) as u8
}

impl Sector {
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
    pub fn generate_subchannel(&self) -> Vec<u8> {
        // The first sector of the disc, and only the first sector,
        // gets an FFed out P sector like a pregap. Every other non-pregap
        // sector uses 0s.
        // For players which ignore the Q subchannel, this allows
        // locating the start of tracks.
        let mut p = if self.start == 0 || self.index.number == 0 {
            vec![0xFF; 12]
        } else {
            vec![0; 12]
        };
        let mut q = Sector::generate_q_subchannel(
            self.absolute_start,
            self.relative_position,
            self.track.number,
            self.index.number,
            self.track.mode,
        );
        // The vast majority of real discs write their unused R-W fields as 0s,
        // but at least one real disc used FFs instead. We'll side with the
        // majority and use 0.
        let mut rest = vec![0; 72];

        let mut out = vec![];
        out.append(&mut p);
        out.append(&mut q);
        out.append(&mut rest);

        out
    }

    fn generate_q_subchannel(
        absolute_sector: i64,
        relative_sector: i64,
        track: u8,
        index: u8,
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
        // Note that the cuesheet *can* contain the catalog number,
        // so it'd be possible for us to set this, but libcue doesn't
        // expose a getter for that; it's simpler just to skip it.
        q[0] |= 1 << 0;
        // OK, it's data time! This is the next 9 bytes.
        // This contains timing info for the current track.
        q[1] = bcd(track as i64);

        // Next is the index. While it supports values up to 99,
        // usually only two values are seen:
        // 00 - Pregap or postgap
        // 01 - First index within the track, or leadout
        q[2] = bcd(index as i64);

        // The next three fields, MIN, SEC, and FRAC, are the
        // running time within each index.
        // FRAC is a unit of 1/75th of a second, e.g. the
        // duration of exactly one sector.
        // In the pregap, this starts at negative the
        // pregap duration and counts up to 0.
        // In the actual content, this starts at 0 and
        // counts up.
        // MIN
        q[3] = bcd(relative_sector / 4500) as u8;
        // SEC
        q[4] = bcd((relative_sector / 75) % 60) as u8;
        // FRAC
        q[5] = bcd(relative_sector % 75) as u8;
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
}

// For more detail, see section 22.3.4.2 of ECMA-130.
enum Pointer {
    Track(u8),
    FirstTrack,
    LastTrack,
    LeadOut,
}

impl Pointer {
    fn as_u8(&self) -> u8 {
        match self {
            Self::Track(value) => *value,
            Self::FirstTrack => 0xA0,
            Self::LastTrack => 0xA1,
            Self::LeadOut => 0xA2,
        }
    }
}
