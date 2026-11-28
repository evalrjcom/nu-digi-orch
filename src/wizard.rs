use crate::song::{Song, Tick};
use crate::{app, flat_label, Button, Choice, Input, IntInput, Window, BG, SUBTLE, TEXT};
use fltk::prelude::*;
use std::{cell::RefCell, rc::Rc};

const PRESETS: &[(&str, &str)] = &[
    ("Four-chord pop", "I V vi IV"),
    ("Sensitive pop", "vi IV I V"),
    ("50s doo-wop", "I vi IV V"),
    ("Pop turnaround", "I vi ii V"),
    ("Axis variation", "I IV vi V"),
    ("Royal road", "IV V iii vi"),
    ("Three-chord rock", "I IV V I"),
    ("Classic rock", "I bVII IV I"),
    ("Pachelbel sequence", "I V vi iii IV I IV V"),
    ("12-bar blues", "I7 I7 I7 I7 IV7 IV7 I7 I7 V7 IV7 I7 V7"),
    (
        "Quick-change blues",
        "I7 IV7 I7 I7 IV7 IV7 I7 I7 V7 IV7 I7 V7",
    ),
    ("Major ii–V–I", "iim7 V7 Imaj7 Imaj7"),
    ("Minor ii–V–i", "iim7b5 V7 i7 i7"),
    ("Jazz turnaround", "Imaj7 vi7 iim7 V7"),
    ("Rhythm changes (A)", "Imaj7 vi7 iim7 V7 iiim7 VI7 iim7 V7"),
    ("Descending fifths", "vi ii V I"),
    ("Andalusian cadence", "i bVII bVI V"),
    ("Minor pop", "i bVI bIII bVII"),
    ("Minor plagal", "i iv i i"),
    ("Dorian vamp", "i IV i IV"),
    ("Mixolydian vamp", "I bVII I bVII"),
    ("Phrygian vamp", "i bII i bII"),
    ("Retro synth", "i bVI bVII i"),
    ("Gospel cadence", "IV I V I"),
    ("Two-chord soul", "Imaj7 iim7 Imaj7 iim7"),
];
const KEYS: [(&str, u8, i8, i8); 12] = [
    ("C", 0, 0, -3),
    ("Db", 1, -5, 4),
    ("D", 2, 2, -1),
    ("Eb", 3, -3, -6),
    ("E", 4, 4, 1),
    ("F", 5, -1, -4),
    ("F#", 6, 6, 3),
    ("G", 7, 1, -2),
    ("Ab", 8, -4, -7),
    ("A", 9, 3, 0),
    ("Bb", 10, -2, -5),
    ("B", 11, 5, 2),
];

struct Style {
    name: &'static str,
    bpm: u16,
    kick: &'static [u8],
    snare: &'static [u8],
    bass: &'static [u8],
    keys: &'static [u8],
    arp_step: u8,
    swing: u8,
    patches: [u8; 4],
}
const STYLES: &[Style] = &[
    Style {
        name: "Pop",
        bpm: 116,
        kick: &[0, 6, 8, 10],
        snare: &[4, 12],
        bass: &[0, 4, 8, 12],
        keys: &[0, 8],
        arp_step: 2,
        swing: 0,
        patches: [33, 89, 4, 0],
    },
    Style {
        name: "Rock",
        bpm: 132,
        kick: &[0, 2, 8, 10],
        snare: &[4, 12],
        bass: &[0, 2, 4, 6, 8, 10, 12, 14],
        keys: &[0, 4, 8, 12],
        arp_step: 4,
        swing: 0,
        patches: [34, 16, 27, 29],
    },
    Style {
        name: "House",
        bpm: 124,
        kick: &[0, 4, 8, 12],
        snare: &[4, 12],
        bass: &[2, 6, 10, 14],
        keys: &[2, 6, 10, 14],
        arp_step: 1,
        swing: 0,
        patches: [38, 90, 81, 4],
    },
    Style {
        name: "Hip-hop",
        bpm: 88,
        kick: &[0, 7, 10],
        snare: &[4, 12],
        bass: &[0, 7, 10],
        keys: &[0, 6, 11],
        arp_step: 4,
        swing: 5,
        patches: [39, 88, 10, 4],
    },
    Style {
        name: "Jazz swing",
        bpm: 144,
        kick: &[0, 8],
        snare: &[4, 12],
        bass: &[0, 4, 8, 12],
        keys: &[2, 7, 12, 15],
        arp_step: 2,
        swing: 60,
        patches: [32, 48, 11, 0],
    },
    Style {
        name: "Reggae",
        bpm: 82,
        kick: &[8],
        snare: &[8],
        bass: &[0, 3, 6, 10, 14],
        keys: &[2, 6, 10, 14],
        arp_step: 4,
        swing: 5,
        patches: [33, 16, 27, 24],
    },
    Style {
        name: "Funk",
        bpm: 108,
        kick: &[0, 3, 7, 10],
        snare: &[4, 12],
        bass: &[0, 3, 6, 8, 11, 14],
        keys: &[1, 4, 7, 10, 14],
        arp_step: 2,
        swing: 5,
        patches: [36, 89, 7, 27],
    },
    Style {
        name: "Ambient",
        bpm: 70,
        kick: &[0],
        snare: &[],
        bass: &[0, 8],
        keys: &[3, 11],
        arp_step: 4,
        swing: 0,
        patches: [38, 91, 8, 46],
    },
    Style {
        name: "Latin",
        bpm: 110,
        kick: &[0, 6, 10],
        snare: &[3, 8, 14],
        bass: &[0, 6, 12],
        keys: &[2, 5, 8, 11, 14],
        arp_step: 2,
        swing: 0,
        patches: [32, 48, 12, 0],
    },
    Style {
        name: "Blues shuffle",
        bpm: 102,
        kick: &[0, 8, 10],
        snare: &[4, 12],
        bass: &[0, 4, 8, 12],
        keys: &[0, 5, 8, 13],
        arp_step: 2,
        swing: 60,
        patches: [33, 18, 26, 0],
    },
];

#[derive(Clone, Copy)]
struct Chord {
    root: u8,
    bass: Option<u8>,
    intervals: &'static [u8],
}
fn parse_chord(token: &str, tonic: u8) -> Result<Chord, String> {
    let fail = || format!("Unsupported chord: {token}");
    let (token, bass) = if let Some((chord, bass)) = token.split_once('/') {
        let first = bass
            .as_bytes()
            .first()
            .copied()
            .filter(|c| matches!(c, b'A'..=b'G'))
            .ok_or_else(fail)?;
        let (shift, remaining) = accidental(&bass[1..]);
        if !remaining.is_empty() {
            return Err(fail());
        }
        let root = [9_i16, 11, 0, 2, 4, 5, 7][usize::from(first - b'A')];
        (chord, Some((root + shift).rem_euclid(12) as u8))
    } else {
        (token, None)
    };
    let first = token.as_bytes().first().copied().ok_or_else(fail)?;
    let (root, suffix, minor) = if matches!(first, b'A'..=b'G') {
        let root = [9, 11, 0, 2, 4, 5, 7][usize::from(first - b'A')];
        let (shift, suffix) = accidental(&token[1..]);
        (root + shift, suffix, false)
    } else {
        let (shift, text) = accidental(token);
        let count = text
            .bytes()
            .take_while(|byte| matches!(byte, b'I' | b'V' | b'i' | b'v'))
            .count();
        let (roman, suffix) = text.split_at(count);
        let degree = match roman.to_ascii_uppercase().as_str() {
            "I" => 0,
            "II" => 2,
            "III" => 4,
            "IV" => 5,
            "V" => 7,
            "VI" => 9,
            "VII" => 11,
            _ => return Err(fail()),
        };
        (
            i16::from(tonic) + degree + shift,
            suffix,
            roman.starts_with(['i', 'v']),
        )
    };
    let intervals: &[u8] = match (suffix, minor) {
        ("", false) | ("maj", _) => &[0, 4, 7],
        ("", true) | ("m" | "min", _) => &[0, 3, 7],
        ("7", false) => &[0, 4, 7, 10],
        ("7", true) | ("m7", _) => &[0, 3, 7, 10],
        ("maj7", _) => &[0, 4, 7, 11],
        ("dim" | "°", _) => &[0, 3, 6],
        ("dim7" | "°7", _) => &[0, 3, 6, 9],
        ("m7b5" | "ø", _) => &[0, 3, 6, 10],
        ("aug" | "+", _) => &[0, 4, 8],
        ("sus2", _) => &[0, 2, 7],
        ("sus" | "sus4", _) => &[0, 5, 7],
        ("6", false) => &[0, 4, 7, 9],
        ("6", true) | ("m6", _) => &[0, 3, 7, 9],
        ("add9", _) => &[0, 4, 7, 14],
        ("9", false) => &[0, 4, 7, 10, 14],
        ("9", true) | ("m9", _) => &[0, 3, 7, 10, 14],
        ("maj9", _) => &[0, 4, 7, 11, 14],
        _ => return Err(fail()),
    };
    Ok(Chord {
        root: root.rem_euclid(12) as u8,
        bass,
        intervals,
    })
}
fn accidental(text: &str) -> (i16, &str) {
    if let Some(rest) = text.strip_prefix(['b', '♭']) {
        (-1, rest)
    } else if let Some(rest) = text.strip_prefix(['#', '♯']) {
        (1, rest)
    } else {
        (0, text)
    }
}

fn generate(text: &str, key: usize, style: &Style, bpm: u16, repeats: u8) -> Result<Song, String> {
    let (_, tonic, major, minor) = KEYS[key];
    let chords: Vec<_> = text
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | ','))
        .filter(|s| !s.is_empty())
        .take(65)
        .map(|s| parse_chord(s, tonic))
        .collect::<Result<_, _>>()?;
    if !(1..=64).contains(&chords.len())
        || !(1..=16).contains(&repeats)
        || !(30..=300).contains(&bpm)
    {
        return Err("Use 1–64 chords, 1–16 repeats, and 30–300 BPM.".into());
    }
    let mut song = Song::new();
    song.set_tempo(f64::from(bpm));
    let first = text.trim_start().trim_start_matches(['b', '#', '♭', '♯']);
    let is_minor = first
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | ','))
        .find(|token| !token.is_empty())
        .and_then(|token| parse_chord(token, tonic).ok())
        .is_some_and(|chord| chord.root == tonic && chord.intervals.get(1) == Some(&3));
    song.set_key(0, (if is_minor { minor } else { major }, is_minor));
    for _ in 0..4 {
        song.add_track();
    }
    for (index, name) in ["Drums", "Bass", "Pad", "Arpeggio", "Rhythm keys"]
        .into_iter()
        .enumerate()
    {
        song.tracks[index].rename(name);
        if index == 0 {
            song.tracks[index].set_channel(9);
        } else {
            song.tracks[index].set_setting(None, style.patches[index - 1]);
        }
        song.tracks[index].set_setting(Some(7), [100, 97, 77, 83, 84][index]);
    }
    let step = Tick::from(song.ppq) / 4;
    for (bar, chord) in chords
        .iter()
        .cycle()
        .take(chords.len() * usize::from(repeats))
        .enumerate()
    {
        let start = bar as Tick * 16 * step;
        let end = start + 16 * step;
        let at = |slot: u8| {
            // Delay alternating subdivisions while keeping every event inside its bar.
            start
                + Tick::from(slot) * step
                + if slot % 4 >= 2 {
                    step * Tick::from(style.swing) / 100
                } else {
                    0
                }
        };
        let tracks = &mut song.tracks;
        for (key, pulses, vel) in [
            (36, style.kick, 108),
            (38, style.snare, 96),
            (42, &[0, 2, 4, 6, 8, 10, 12, 14][..], 71),
        ] {
            for &pulse in pulses {
                let on = at(pulse);
                tracks[0].add_note(
                    on,
                    on + step / 2,
                    key,
                    if pulse % 4 == 0 { vel } else { vel - 10 },
                );
            }
        }
        for (i, &pulse) in style.bass.iter().enumerate() {
            let tone = if i % 4 == 2 { chord.intervals[2] } else { 0 };
            let on = at(pulse);
            let next = style.bass.get(i + 1).map_or(end, |&slot| at(slot));
            tracks[1].add_note(
                on,
                on + (next - on) * 3 / 4,
                36 + if i % 4 == 2 {
                    (chord.root + tone) % 12
                } else {
                    chord.bass.unwrap_or(chord.root)
                },
                95,
            );
        }
        for &tone in chord.intervals {
            tracks[2].add_note(start, end, 48 + chord.root + tone, 68);
            for &pulse in style.keys {
                let on = at(pulse);
                tracks[4].add_note(on, (on + step * 3 / 4).min(end), 60 + chord.root + tone, 79);
            }
        }
        for (i, pulse) in (0..16).step_by(usize::from(style.arp_step)).enumerate() {
            let on = at(pulse);
            let key = 60 + chord.root + chord.intervals[i % chord.intervals.len()];
            tracks[3].add_note(
                on,
                (on + step * Tick::from(style.arp_step) * 2 / 3).min(end),
                key,
                81,
            );
        }
    }
    Ok(song)
}

pub(super) fn show() -> Option<Song> {
    let mut window = Window::default()
        .with_size(550, 308)
        .center_screen()
        .with_label("New song from progression");
    window.set_color(BG);
    window.make_modal(true);
    flat_label(18, 16, 120, 26, "Progression preset", 13, TEXT);
    let mut preset = Choice::new(144, 16, 388, 28, None);
    for &(name, _) in PRESETS {
        preset.add_choice(name);
    }
    preset.set_value(0);
    flat_label(18, 58, 116, 26, "Chord sequence", 13, TEXT);
    let mut progression = Input::new(144, 58, 388, 28, None);
    progression.set_value(PRESETS[0].1);
    flat_label(
        144,
        88,
        388,
        22,
        "One chord per bar · e.g. I V vi IV or Dm7 G7 Cmaj7",
        12,
        SUBTLE,
    );
    let mut editor = progression.clone();
    preset.set_callback(move |p| {
        if let Some(&(_, text)) = PRESETS.get(p.value() as usize) {
            editor.set_value(text);
        }
    });
    flat_label(18, 121, 116, 26, "Tonic", 13, TEXT);
    let mut key = Choice::new(144, 121, 90, 28, None);
    for &(name, _, _, _) in &KEYS {
        key.add_choice(name);
    }
    key.set_value(0);
    flat_label(18, 162, 116, 26, "Genre / style", 13, TEXT);
    let mut style = Choice::new(144, 162, 195, 28, None);
    for item in STYLES {
        style.add_choice(item.name);
    }
    style.set_value(0);
    flat_label(18, 204, 116, 26, "Tempo (BPM)", 13, TEXT);
    let mut tempo = IntInput::new(144, 204, 83, 28, None);
    tempo.set_value(&STYLES[0].bpm.to_string());
    let mut field = tempo.clone();
    style.set_callback(move |s| {
        if let Some(style) = STYLES.get(s.value() as usize) {
            field.set_value(&style.bpm.to_string());
        }
    });
    flat_label(258, 204, 88, 26, "Repeats", 13, TEXT);
    let mut repeats = IntInput::new(327, 204, 62, 28, None);
    repeats.set_value("2");
    let mut cancel = Button::new(280, 259, 101, 30, "Cancel");
    let mut create = Button::new(392, 259, 140, 30, "Create song");
    create.set_color(crate::ACCENT);
    create.set_label_color(BG);
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let mut win = window.clone();
    create.set_callback(move |_| {
        let Some(style) = STYLES.get(style.value() as usize) else {
            return;
        };
        match generate(
            &progression.value(),
            key.value().max(0) as usize,
            style,
            tempo.value().trim().parse().unwrap_or(0),
            repeats.value().trim().parse().unwrap_or(0),
        ) {
            Ok(song) => {
                *output.borrow_mut() = Some(song);
                win.hide();
            }
            Err(error) => fltk::dialog::alert_default(&error),
        }
    });
    let mut win = window.clone();
    cancel.set_callback(move |_| win.hide());
    window.end();
    window.show();
    while window.shown() && app::wait() {}
    let song = result.borrow_mut().take();
    app::delete_widget(window);
    song
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_and_styles_generate_editable_parts() {
        for &(_, preset) in PRESETS {
            for style in STYLES {
                let song = generate(preset, 2, style, style.bpm, 1).unwrap();
                assert_eq!(song.tracks.len(), 5);
                assert_eq!(song.tracks[0].channel, 9);
                assert!(song.tracks.iter().all(|t| !t.notes().is_empty()));
            }
        }
        assert_eq!(parse_chord("bVII7", 2).unwrap().root, 0);
        assert_eq!(parse_chord("F♯m7", 0).unwrap().root, 6);
        assert_eq!(parse_chord("C/E", 0).unwrap().bass, Some(4));
        assert!(generate("bad chord", 0, &STYLES[0], 120, 1).is_err());
        let jazz = generate("ii7 V7 Imaj7", 0, &STYLES[4], 144, 2).unwrap();
        assert_eq!(jazz.end_tick(), 6 * 4 * Tick::from(jazz.ppq));
    }
}
