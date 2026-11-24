use crate::song::{Packet, Song, Timeline};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default)]
pub enum Pattern {
    #[default]
    Click,
    Rock,
    FourOnFloor,
    HipHop,
    Shuffle,
}

impl Pattern {
    pub const ALL: [Self; 5] = [
        Self::Click,
        Self::Rock,
        Self::FourOnFloor,
        Self::HipHop,
        Self::Shuffle,
    ];
    pub const LABELS: &str = "Click|Rock|Four-on-floor|Hip-hop|Shuffle";

    fn hits(self, step: u64) -> [(u8, u8); 3] {
        let (kick, snare, hat) = match self {
            Self::Click => {
                return [
                    if step == 0 {
                        (76, 115)
                    } else if step.is_multiple_of(6) {
                        (77, 90)
                    } else {
                        (0, 0)
                    },
                    (0, 0),
                    (0, 0),
                ]
            }
            Self::Rock => (
                matches!(step, 0 | 12 | 15),
                matches!(step, 6 | 18),
                step.is_multiple_of(3),
            ),
            Self::FourOnFloor => (
                step.is_multiple_of(6),
                matches!(step, 6 | 18),
                step % 6 == 3,
            ),
            Self::HipHop => (
                matches!(step, 0 | 9 | 12 | 21),
                matches!(step, 6 | 18),
                step.is_multiple_of(3),
            ),
            Self::Shuffle => (
                matches!(step, 0 | 12),
                matches!(step, 6 | 18),
                matches!(step % 6, 0 | 4),
            ),
        };
        [
            (36, if kick { 105 } else { 0 }),
            (38, if snare { 100 } else { 0 }),
            (
                42,
                if hat {
                    if step.is_multiple_of(6) {
                        75
                    } else {
                        55
                    }
                } else {
                    0
                },
            ),
        ]
    }
}

pub const NOTES: [u8; 5] = [36, 38, 42, 76, 77];

// Reserve the SoundFont stem even when switched off, so live enabling needs no new audio stream.
pub fn append(timeline: &mut Timeline, song: &Song, pattern: Option<Pattern>) {
    let track = timeline.buses.len();
    timeline.buses.push(Arc::default());
    timeline.instrument_tracks.push(track);
    let Some(pattern) = pattern else {
        return;
    };
    let clock = song.tempo_clock();
    let ppq = u64::from(song.ppq);
    let steps = (u128::from(clock(timeline.duration)) * 6 / u128::from(ppq)).min(720_000) as u64;
    let mut previous_tick = None;
    for step in 0..=steps {
        let tick = step * ppq / 6;
        let hits = pattern.hits(step % 24);
        if hits.iter().all(|&(_, velocity)| velocity == 0) {
            continue;
        }
        if previous_tick == Some(tick) {
            continue;
        }
        previous_tick = Some(tick);
        let time = super::tick_time(&clock, tick, timeline.duration);
        if time >= timeline.duration {
            break;
        }
        for (key, velocity) in hits {
            if velocity == 0 {
                continue;
            }
            timeline.packets.push(Packet {
                track,
                micros: time,
                bytes: [0x99, key, velocity],
                len: 3,
            });
            timeline.packets.push(Packet {
                track,
                micros: time.saturating_add(36_000).min(timeline.duration),
                bytes: [0x89, key, 0],
                len: 3,
            });
        }
    }
    timeline.packets.sort_by_key(|p| p.micros);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_reserves_the_same_stem_and_patterns_use_tempo() {
        let mut song = Song::demo();
        song.set_tempo(60.);
        let mut off = song.timeline();
        let count = off.packets.len();
        append(&mut off, &song, None);
        assert_eq!(off.packets.len(), count);
        let mut rock = song.timeline();
        append(&mut rock, &song, Some(Pattern::Rock));
        assert_eq!(off.instrument_tracks, rock.instrument_tracks);
        let track = *rock.instrument_tracks.last().unwrap();
        assert!(rock
            .packets
            .iter()
            .any(|p| p.track == track && p.micros == 1_000_000 && p.bytes[1] == 38));
        assert!(rock
            .packets
            .iter()
            .any(|p| p.track == track && p.micros == 500_000 && p.bytes[1] == 42));
    }
}
