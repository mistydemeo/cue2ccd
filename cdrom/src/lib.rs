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
                    let relative_position = sector - track.start as i64;

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
#[derive(Clone, Debug)]
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
