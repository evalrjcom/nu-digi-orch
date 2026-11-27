use crate::song::{Song, Tick};

pub const MAJOR: [&str; 15] = [
    "Cb", "Gb", "Db", "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#",
];
pub const MINOR: [&str; 15] = [
    "Ab", "Eb", "Bb", "F", "C", "G", "D", "A", "E", "B", "F#", "C#", "G#", "D#", "A#",
];

pub fn key_name(key: (i8, bool)) -> String {
    let (accidentals, minor) = key;
    let names = if minor { MINOR } else { MAJOR };
    let name = names
        .get((i16::from(accidentals) + 7) as usize)
        .unwrap_or(&"?");
    format!("{name}{}", if minor { " minor" } else { " major" })
}

pub fn transpose_key(accidentals: i8, minor: bool, semitones: i8) -> i8 {
    if semitones.rem_euclid(12) == 0 {
        return accidentals;
    }
    let target = (7 * i16::from(accidentals) + i16::from(semitones)).rem_euclid(12);
    (-7_i8..=7)
        .filter(|&n| (7 * i16::from(n)).rem_euclid(12) == target)
        .min_by_key(|&n| {
            (
                n.abs(),
                if minor {
                    n.is_negative()
                } else {
                    n.is_positive()
                },
            )
        })
        .unwrap_or(accidentals)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chord(u8, &'static str);
impl Chord {
    pub fn label(self, flats: bool) -> String {
        let names = if flats {
            [
                "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
            ]
        } else {
            [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ]
        };
        format!("{}{}", names[self.0 as usize], self.1)
    }
}

pub(super) fn detect(mask: u16, bass: u8) -> Option<Chord> {
    for distance in 0..12 {
        let root = (bass + distance) % 12;
        let shifted = ((mask >> root) | (mask << (12 - root))) & 0xfff;
        for (intervals, name) in [
            (&[0, 4, 7, 11][..], "maj7"),
            (&[0, 4, 7, 10], "7"),
            (&[0, 3, 7, 10], "m7"),
            (&[0, 3, 6, 10], "m7b5"),
            (&[0, 3, 6, 9], "dim7"),
            (&[0, 4, 7], ""),
            (&[0, 3, 7], "m"),
            (&[0, 3, 6], "dim"),
            (&[0, 4, 8], "aug"),
            (&[0, 2, 7], "sus2"),
            (&[0, 5, 7], "sus4"),
        ] {
            if intervals.iter().fold(0, |bits, &n| bits | (1 << n)) == shifted {
                return Some(Chord(root, name));
            }
        }
    }
    None
}

pub fn progression(song: &Song) -> Vec<(Tick, Option<Chord>)> {
    let solo = song.tracks.iter().any(|track| track.solo);
    let mut events = Vec::new();
    for track in song.tracks.iter().filter(|t| !t.mute && (!solo || t.solo)) {
        for note in track
            .notes()
            .into_iter()
            .filter(|n| n.channel != 9 && n.end > n.start)
        {
            events.push((note.start, note.key, true));
            events.push((note.end, note.key, false));
        }
    }
    events.sort_unstable();
    let mut counts = [0_usize; 128];
    let mut result = Vec::new();
    for group in events.chunk_by(|a, b| a.0 == b.0) {
        for &(_, key, on) in group {
            let slot = &mut counts[key as usize];
            if on {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
        let mut mask = 0;
        let mut bass = None;
        for (key, _) in counts.iter().enumerate().filter(|(_, n)| **n > 0) {
            let class = (key % 12) as u8;
            bass.get_or_insert(class);
            mask |= 1 << class;
        }
        let chord = bass.and_then(|bass| detect(mask, bass));
        if result.last().is_none_or(|&(_, old)| old != chord) {
            result.push((group[0].0, chord));
        }
    }
    result
}

// Draw each recognized change at its musical tick, including the chord carried into this page.
pub(super) fn draw_ruler(frame: &super::Frame, model: &super::Model, gutter: i32) {
    use fltk::{
        draw,
        enums::{Align, Font},
        prelude::WidgetExt,
    };
    let left = frame.x() + gutter;
    let width = frame.w() - gutter;
    let top = frame.y();
    let height = super::CHORD_BAND;
    let ppq = u64::from(model.song.ppq);
    let start = model.view_beat * ppq;
    let span = super::BEATS * ppq;
    let end = start + span;
    let x_for = |tick| left + ((tick - start) * width as u64 / span) as i32;
    draw::push_clip(left, top, width, height);
    draw::draw_rect_fill(left, top, width, height, super::BG);
    draw::set_font(Font::Helvetica, 12);
    let first = model
        .chords
        .partition_point(|&(tick, _)| tick <= start)
        .saturating_sub(1);
    for (index, &(tick, chord)) in model
        .chords
        .iter()
        .enumerate()
        .skip(first)
        .take_while(|(_, entry)| entry.0 < end)
    {
        let Some(chord) = chord else {
            continue;
        };
        let at = tick.max(start);
        let x = x_for(at);
        let next = model.chords[index + 1..]
            .iter()
            .find(|(_, next)| next.is_some())
            .map_or(end, |entry| entry.0.min(end));
        let room = x_for(next) - x;
        if room < 10 {
            continue;
        }
        let key_index = model.keys.partition_point(|&(at_key, _)| at_key <= at);
        let flats = key_index
            .checked_sub(1)
            .is_some_and(|i| model.keys[i].1 .0 < 0);
        draw::draw_rect_fill(x, top + 3, 1, height - 6, super::ACCENT);
        draw::set_draw_color(super::ACCENT);
        draw::draw_text2(
            &chord.label(flats),
            x + 3,
            top,
            room - 4,
            height,
            Align::Inside | Align::Left | Align::Clip,
        );
    }
    draw::pop_clip();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_inverted_major_and_dominant_seventh() {
        assert_eq!(
            detect((1 << 0) | (1 << 4) | (1 << 7), 4),
            Some(Chord(0, ""))
        );
        assert_eq!(
            detect((1 << 0) | (1 << 4) | (1 << 7) | (1 << 10), 7),
            Some(Chord(0, "7"))
        );
    }
}
