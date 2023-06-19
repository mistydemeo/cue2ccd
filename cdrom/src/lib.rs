use cdrom_crc::{crc16, CRC16_INITIAL_CRC};
use cue::cd::CD;
use cue::track;

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
                    next.start as i64
                } else {
                    track.start + track.length
                };

                if index.start as i64 <= sector && boundary >= sector {
                    // Yes, it's okay for this to be negative! Pregap counts backwards
                    // to the start of the following index.
                    let relative_position = sector - track.start;

                    return Some(Sector {
                        start: sector,
                        // Convenience for indexing relative to the start of the disc,
                        // rather than the start of the disc image.
                        // Yes, it means the first sector isn't sector 1.
                        absolute_start: sector + 151,
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
    pub number: usize,
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
}

#[derive(Clone, Debug)]
pub struct Index {
    // Number of the current index; index 0 is the pregap, index 1 onward are the track proper
    pub number: usize,
    // Start of the current index, in sectors
    pub start: isize,
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

fn bcd(dec: i64) -> i64 {
    ((dec / 10) << 4) | (dec % 10)
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
        track: usize,
        index: usize,
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
        q[1] = track as u8;

        // Next is the index. While it supports values up to 99,
        // usually only two values are seen:
        // 00 - Pregap or postgap
        // 01 - First index within the track, or leadout
        q[2] = index as u8;

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
