// Indexed by the MIDI Program Change byte (0–127).
const INSTRUMENTS: [&str; 128] = [
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavinet",
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    "Nylon Guitar",
    "Steel Guitar",
    "Jazz Guitar",
    "Clean Guitar",
    "Muted Guitar",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    "Acoustic Bass",
    "Fingered Electric Bass",
    "Picked Electric Bass",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    "Square Lead",
    "Sawtooth Lead",
    "Calliope Lead",
    "Chiff Lead",
    "Charang Lead",
    "Voice Lead",
    "Fifths Lead",
    "Bass and Lead",
    "New Age Pad",
    "Warm Pad",
    "Polysynth Pad",
    "Choir Pad",
    "Bowed Pad",
    "Metallic Pad",
    "Halo Pad",
    "Sweep Pad",
    "Rain FX",
    "Soundtrack FX",
    "Crystal FX",
    "Atmosphere FX",
    "Brightness FX",
    "Goblins FX",
    "Echoes FX",
    "Sci-Fi FX",
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bag Pipe",
    "Fiddle",
    "Shanai",
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

pub(super) fn instrument(track: &crate::song::Track) -> &'static str {
    if track.channel == 9 {
        "Drum Kit"
    } else if track.setting(Some(0)) != 0 {
        "Bank Preset"
    } else {
        INSTRUMENTS[usize::from(track.setting(None))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_follow_patch_bank_and_channel() {
        let mut track = crate::song::Track::default();
        assert_eq!(instrument(&track), "Acoustic Grand Piano");
        track.set_setting(None, 40);
        assert_eq!(instrument(&track), "Violin");
        track.set_setting(None, 127);
        assert_eq!(instrument(&track), "Gunshot");
        track.set_setting(Some(0), 1);
        assert_eq!(instrument(&track), "Bank Preset");
        track.set_channel(9);
        assert_eq!(instrument(&track), "Drum Kit");
    }
}
