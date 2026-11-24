use midly::{
    num::{u15, u24, u28, u4, u7},
    Format, Header, MetaMessage, MidiMessage, PitchBend, Smf, Timing, TrackEvent, TrackEventKind,
};
use std::{
    collections::{HashMap, VecDeque},
    fs, io,
    path::Path,
};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub type Tick = u64;
const MAX_DELTA: Tick = 0x0fff_ffff;

#[derive(Clone)]
enum Event {
    Midi(u8, MidiMessage),
    Meta(MetaMessage<'static>),
    Text(u8, Vec<u8>),
    SysEx(Vec<u8>),
    Escape(Vec<u8>),
}
impl Event {
    fn from_midi(kind: TrackEventKind<'_>) -> Self {
        use MetaMessage::*;
        match kind {
            TrackEventKind::Midi { channel, message } => Self::Midi(channel.as_int(), message),
            TrackEventKind::SysEx(bytes) => Self::SysEx(bytes.to_vec()),
            TrackEventKind::Escape(bytes) => Self::Escape(bytes.to_vec()),
            TrackEventKind::Meta(meta) => match meta {
                Text(b) => Self::Text(1, b.to_vec()),
                Copyright(b) => Self::Text(2, b.to_vec()),
                TrackName(b) => Self::Text(3, b.to_vec()),
                InstrumentName(b) => Self::Text(4, b.to_vec()),
                Lyric(b) => Self::Text(5, b.to_vec()),
                Marker(b) => Self::Text(6, b.to_vec()),
                CuePoint(b) => Self::Text(7, b.to_vec()),
                ProgramName(b) => Self::Text(8, b.to_vec()),
                DeviceName(b) => Self::Text(9, b.to_vec()),
                SequencerSpecific(b) => Self::Text(0x7f, b.to_vec()),
                Unknown(id, b) => Self::Text(id, b.to_vec()),
                _ => Self::Meta(meta.to_static()),
            },
        }
    }

    fn as_midi(&self) -> TrackEventKind<'_> {
        match self {
            Self::Midi(channel, message) => TrackEventKind::Midi {
                channel: u4::from(*channel),
                message: *message,
            },
            Self::Meta(meta) => TrackEventKind::Meta(*meta),
            Self::Text(id, bytes) => TrackEventKind::Meta(MetaMessage::Unknown(*id, bytes)),
            Self::SysEx(bytes) => TrackEventKind::SysEx(bytes),
            Self::Escape(bytes) => TrackEventKind::Escape(bytes),
        }
    }
}

#[derive(Clone, Default)]
pub struct Track {
    pub bus: std::sync::Arc<crate::bus::Bus>,
    events: Vec<(Tick, Event)>,
    end: Tick,
    inherited: Option<(u8, [Option<u8>; 4])>,
    pub mute: bool,
    pub solo: bool,
    pub channel: u8,
}
#[derive(Clone)]
pub struct Song {
    pub transposed: i32,
    pub ppq: u16,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note {
    pub on: usize,
    pub off: usize,
    pub start: Tick,
    pub end: Tick,
    pub key: u8,
    pub velocity: u8,
    pub channel: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Packet {
    pub track: usize,
    pub micros: u64,
    pub bytes: [u8; 3],
    pub len: usize,
}
pub struct Timeline {
    pub instrument_tracks: Vec<usize>,
    pub buses: Vec<std::sync::Arc<crate::bus::Bus>>,
    pub packets: Vec<Packet>,
    pub duration: u64,
}

impl Timeline {
    pub fn seek(&mut self, offset: u64) {
        // Clamp a stale cue after edits shorten the arrangement.
        let offset = offset.min(self.duration);
        if offset == 0 {
            return;
        }
        let mut notes = std::collections::BTreeMap::new();
        let mut events = Vec::new();
        for mut event in self
            .packets
            .drain(..self.packets.partition_point(|p| p.micros < offset))
        {
            let [status, key, value] = event.bytes;
            let channel = usize::from(status & 15);
            match status & 0xf0 {
                0x80 => {
                    notes.remove(&(event.track, channel, key));
                }
                0x90 => {
                    if value > 0 {
                        notes.insert((event.track, channel, key), value);
                    } else {
                        notes.remove(&(event.track, channel, key));
                    }
                }
                _ => {
                    if status & 0xf0 == 0xb0 && matches!(key, 120 | 123) {
                        notes.retain(|(_, ch, _), _| *ch != channel);
                    }
                    event.micros = 0;
                    events.push(event);
                }
            }
        }
        for ((track, channel, key), velocity) in notes {
            events.push(Packet {
                track,
                micros: 0,
                bytes: [0x90 | channel as u8, key, velocity],
                len: 3,
            });
        }
        for event in &mut self.packets {
            event.micros -= offset;
        }
        events.append(&mut self.packets);
        self.packets = events;
        self.duration -= offset;
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

const INITIAL_SETTINGS: [Option<u8>; 4] = [None, Some(0), Some(7), Some(10)];

fn setting_value(message: MidiMessage, cc: Option<u8>) -> Option<u8> {
    match message {
        MidiMessage::ProgramChange { program } if cc.is_none() => Some(program.as_int()),
        MidiMessage::Controller { controller, value } if cc == Some(controller.as_int()) => {
            Some(value.as_int())
        }
        _ => None,
    }
}

impl Track {
    pub fn name(&self) -> String {
        self.events
            .iter()
            .find_map(|(_, event)| match event {
                Event::Text(3, bytes) => {
                    Some(String::from_utf8_lossy(bytes).replace(['\r', '\n', '\t'], " "))
                }
                _ => None,
            })
            .unwrap_or_else(|| "Untitled".into())
    }

    pub fn rename(&mut self, name: &str) {
        self.events
            .retain(|(_, event)| !matches!(event, Event::Text(3, _)));
        self.events
            .insert(0, (0, Event::Text(3, name.as_bytes().to_vec())));
    }

    pub fn notes(&self) -> Vec<Note> {
        let mut indexes: Vec<_> = (0..self.events.len()).collect();
        indexes.sort_by_key(|&index| self.events[index].0);
        let mut active: HashMap<(u8, u8), VecDeque<(usize, u8)>> = HashMap::new();
        let mut notes = Vec::new();
        for index in indexes {
            let (tick, Event::Midi(channel, message)) = self.events[index] else {
                continue;
            };
            let (key, velocity) = match message {
                MidiMessage::NoteOn { key, vel } => (key.as_int(), vel.as_int()),
                MidiMessage::NoteOff { key, .. } => (key.as_int(), 0),
                _ => continue,
            };
            if velocity > 0 {
                active
                    .entry((channel, key))
                    .or_default()
                    .push_back((index, velocity));
            } else if let Some((on, velocity)) = active
                .get_mut(&(channel, key))
                .and_then(VecDeque::pop_front)
            {
                notes.push(Note {
                    on,
                    off: index,
                    start: self.events[on].0,
                    end: tick,
                    key,
                    velocity,
                    channel,
                });
            }
        }
        notes
    }

    pub fn add_note(&mut self, start: Tick, end: Tick, key: u8, velocity: u8) -> Note {
        let note = Note {
            on: self.events.len(),
            off: self.events.len() + 1,
            start,
            end,
            key,
            velocity,
            channel: self.channel,
        };
        self.events.push((
            start,
            Event::Midi(
                self.channel,
                MidiMessage::NoteOn {
                    key: key.into(),
                    vel: velocity.into(),
                },
            ),
        ));
        self.events.push((
            end,
            Event::Midi(
                self.channel,
                MidiMessage::NoteOff {
                    key: key.into(),
                    vel: 0.into(),
                },
            ),
        ));
        note
    }

    pub fn change_note(&mut self, note: Note) {
        self.events[note.on] = (
            note.start,
            Event::Midi(
                note.channel,
                MidiMessage::NoteOn {
                    key: note.key.into(),
                    vel: note.velocity.into(),
                },
            ),
        );
        let release = match self.events[note.off].1 {
            Event::Midi(_, MidiMessage::NoteOff { vel, .. }) => vel,
            _ => 0.into(),
        };
        self.events[note.off] = (
            note.end,
            Event::Midi(
                note.channel,
                MidiMessage::NoteOff {
                    key: note.key.into(),
                    vel: release,
                },
            ),
        );
    }

    pub fn delete_note(&mut self, note: Note) {
        self.events.remove(note.on.max(note.off));
        self.events.remove(note.on.min(note.off));
    }

    pub fn set_channel(&mut self, channel: u8) {
        for (_, event) in &mut self.events {
            if let Event::Midi(existing, _) = event {
                if *existing == self.channel {
                    *existing = channel;
                }
            }
        }
        self.channel = channel;
    }

    fn first_note(&self) -> Option<(Tick, usize)> {
        self.events
            .iter()
            .enumerate()
            .filter_map(|(index, (tick, event))| match event {
                Event::Midi(channel, MidiMessage::NoteOn { vel, .. })
                    if *channel == self.channel && vel.as_int() > 0 =>
                {
                    Some((*tick, index))
                }
                _ => None,
            })
            .min()
    }

    fn setting_event(&self, cc: Option<u8>) -> Option<(Tick, usize, u8)> {
        let cutoff = self.first_note();
        self.events
            .iter()
            .enumerate()
            .filter_map(|(index, (tick, event))| {
                if cutoff.is_some_and(|first| (*tick, index) >= first) {
                    return None;
                }
                let Event::Midi(channel, message) = event else {
                    return None;
                };
                if *channel != self.channel {
                    return None;
                }
                setting_value(*message, cc).map(|value| (*tick, index, value))
            })
            .max()
    }

    pub fn has_midi(&self) -> bool {
        self.events
            .iter()
            .any(|(_, event)| matches!(event, Event::Midi(..)))
    }

    pub fn setting(&self, cc: Option<u8>) -> u8 {
        self.setting_event(cc)
            .map(|(_, _, value)| value)
            .or_else(|| {
                let (channel, settings) = self.inherited?;
                if channel != self.channel {
                    return None;
                }
                let index = INITIAL_SETTINGS
                    .iter()
                    .position(|candidate| *candidate == cc)?;
                settings[index]
            })
            .unwrap_or(match cc {
                Some(7) => 100,
                Some(10) => 64,
                _ => 0,
            })
    }

    pub fn set_setting(&mut self, cc: Option<u8>, value: u8) {
        let message = match cc {
            None => MidiMessage::ProgramChange {
                program: value.into(),
            },
            Some(controller) => MidiMessage::Controller {
                controller: controller.into(),
                value: value.into(),
            },
        };
        // Edit the program that actually initializes the track and leave later changes intact.
        if let Some((_, index, _)) = self.setting_event(cc) {
            self.events[index].1 = Event::Midi(self.channel, message);
            return;
        }
        let (tick, index) = if cc == Some(0) {
            self.setting_event(None)
                .map(|(tick, index, _)| (tick, index))
                .or_else(|| self.first_note())
                .unwrap_or((0, 0))
        } else {
            self.first_note().unwrap_or((0, 0))
        };
        self.events
            .insert(index, (tick, Event::Midi(self.channel, message)));
    }
}

impl Song {
    pub fn new() -> Self {
        let mut song = Self {
            transposed: 0,
            ppq: 480,
            tracks: Vec::new(),
        };
        song.add_track();
        song.set_tempo(120.0);
        song
    }

    pub fn demo() -> Self {
        let mut song = Self::new();
        song.tracks[0].rename("Piano");
        for (index, key) in [60, 64, 67, 72, 71, 67, 64, 62, 60].into_iter().enumerate() {
            let start = index as Tick * 240;
            song.tracks[0].add_note(start, start + 220, key, 94);
        }
        song.add_track();
        let bass = &mut song.tracks[1];
        bass.rename("Bass");
        bass.set_setting(None, 32);
        for (index, key) in [48, 48, 43, 43, 48].into_iter().enumerate() {
            let start = index as Tick * 480;
            bass.add_note(start, start + 420, key, 86);
        }
        song
    }

    pub fn add_track(&mut self) {
        let index = self.tracks.len();
        let mut track = Track {
            channel: (0..16)
                .filter(|&channel| channel != 9)
                .find(|channel| self.tracks.iter().all(|track| track.channel != *channel))
                .unwrap_or(0),
            ..Track::default()
        };
        track.rename(&format!("Track {}", index + 1));
        self.tracks.push(track);
    }

    pub fn add_recording(
        &mut self,
        duration: u64,
        events: Vec<(u64, [u8; 3])>,
        offset: u64,
        target: Option<usize>,
    ) -> usize {
        let index = target.unwrap_or(self.tracks.len());
        let mut track = Track::default();
        track.rename(&format!("Take {}", index + 1));
        track.channel = events[0].1[0] & 15;
        let mut active = [[0_u32; 128]; 16];
        let clock = self.tempo_clock();
        for (micros, [status, a, b]) in events {
            let channel = target.map_or(status & 15, |index| self.tracks[index].channel);
            let a = u7::from(a);
            let b = u7::from(b);
            let count = &mut active[usize::from(channel)][usize::from(a.as_int())];
            let message = match status & 0xf0 {
                0x80 => {
                    *count = count.saturating_sub(1);
                    MidiMessage::NoteOff { key: a, vel: b }
                }
                0x90 => {
                    *count = if b.as_int() > 0 {
                        count.saturating_add(1)
                    } else {
                        count.saturating_sub(1)
                    };
                    MidiMessage::NoteOn { key: a, vel: b }
                }
                0xa0 => MidiMessage::Aftertouch { key: a, vel: b },
                0xb0 => MidiMessage::Controller {
                    controller: a,
                    value: b,
                },
                0xc0 => MidiMessage::ProgramChange { program: a },
                0xd0 => MidiMessage::ChannelAftertouch { vel: a },
                0xe0 => MidiMessage::PitchBend {
                    bend: PitchBend(midly::num::u14::from(
                        (u16::from(b.as_int()) << 7) | u16::from(a.as_int()),
                    )),
                },
                _ => continue,
            };
            track.end = clock(offset.saturating_add(micros));
            track
                .events
                .push((track.end, Event::Midi(channel, message)));
        }
        track.end = (track.end + 1).max(clock(offset.saturating_add(duration)));
        // Finish held keys and pedals even when the input is unplugged mid-take.
        let mut used = [false; 16];
        for (_, event) in &track.events {
            if let Event::Midi(channel, _) = event {
                used[usize::from(*channel)] = true;
            }
        }
        for (channel, keys) in active
            .iter()
            .enumerate()
            .filter(|(channel, _)| used[*channel])
        {
            for (key, count) in keys.iter().enumerate() {
                for _ in 0..*count {
                    track.events.push((
                        track.end,
                        Event::Midi(
                            channel as u8,
                            MidiMessage::NoteOff {
                                key: (key as u8).into(),
                                vel: 0.into(),
                            },
                        ),
                    ));
                }
            }
            track.events.push((
                track.end,
                Event::Midi(
                    channel as u8,
                    MidiMessage::Controller {
                        controller: 64.into(),
                        value: 0.into(),
                    },
                ),
            ));
        }
        if let Some(target) = target {
            let existing = &mut self.tracks[target];
            existing.end = existing.end.max(track.end);
            existing.events.extend(
                track
                    .events
                    .into_iter()
                    .filter(|(_, event)| matches!(event, Event::Midi(..))),
            );
        } else {
            self.tracks.push(track);
        }
        index
    }

    pub fn beat_micros_at(&self, position: Tick) -> u64 {
        self.tracks
            .iter()
            .flat_map(|track| &track.events)
            .filter_map(|(tick, event)| match event {
                Event::Meta(MetaMessage::Tempo(value))
                    if *tick <= position && value.as_int() != 0 =>
                {
                    Some((*tick, u64::from(value.as_int())))
                }
                _ => None,
            })
            .max_by_key(|entry| entry.0)
            .map_or(500_000, |entry| entry.1)
    }

    pub fn end_tick(&self) -> Tick {
        self.tracks
            .iter()
            .map(|track| {
                track
                    .events
                    .iter()
                    .map(|(tick, _)| *tick)
                    .max()
                    .unwrap_or(0)
                    .max(track.end)
            })
            .max()
            .unwrap_or(0)
    }

    pub fn key_map(&self) -> Vec<(Tick, (i8, bool))> {
        let mut keys: Vec<_> = self
            .tracks
            .iter()
            .flat_map(|track| &track.events)
            .filter_map(|(tick, event)| match event {
                Event::Meta(MetaMessage::KeySignature(n, minor)) => Some((*tick, (*n, *minor))),
                _ => None,
            })
            .collect();
        keys.sort_by_key(|entry| entry.0);
        keys
    }

    pub fn set_key(&mut self, tick: Tick, key: (i8, bool)) {
        for track in &mut self.tracks {
            track.events.retain(|(at, event)| {
                *at != tick || !matches!(event, Event::Meta(MetaMessage::KeySignature(..)))
            });
        }
        self.tracks[0]
            .events
            .push((tick, Event::Meta(MetaMessage::KeySignature(key.0, key.1))));
    }

    pub fn transpose(&mut self, semitones: i8) -> Result<()> {
        let pitch = |key: u7| i16::from(key.as_int()) + i16::from(semitones);
        for (_, event) in self.tracks.iter().flat_map(|track| &track.events) {
            if let Event::Midi(
                channel,
                MidiMessage::NoteOn { key, .. }
                | MidiMessage::NoteOff { key, .. }
                | MidiMessage::Aftertouch { key, .. },
            ) = event
            {
                if *channel != 9 && !(0..=127).contains(&pitch(*key)) {
                    return Err(invalid(
                        "Transposition would move a note outside MIDI pitches 0–127.",
                    )
                    .into());
                }
            }
        }
        for (_, event) in self.tracks.iter_mut().flat_map(|track| &mut track.events) {
            match event {
                Event::Midi(
                    channel,
                    MidiMessage::NoteOn { key, .. }
                    | MidiMessage::NoteOff { key, .. }
                    | MidiMessage::Aftertouch { key, .. },
                ) if *channel != 9 => {
                    *key = (pitch(*key) as u8).into();
                }
                Event::Meta(MetaMessage::KeySignature(n, minor)) => {
                    *n = crate::harmony::transpose_key(*n, *minor, semitones);
                }
                _ => {}
            }
        }
        self.transposed = self.transposed.saturating_add(i32::from(semitones));
        Ok(())
    }

    pub fn tempo_clock(&self) -> impl Fn(u64) -> Tick + 'static {
        let mut tempos: Vec<_> = self
            .tracks
            .iter()
            .flat_map(|track| &track.events)
            .filter_map(|(tick, event)| match event {
                Event::Meta(MetaMessage::Tempo(value)) if value.as_int() != 0 => {
                    Some((*tick, value.as_int()))
                }
                _ => None,
            })
            .collect();
        tempos.sort_by_key(|&(tick, _)| tick);
        // Store time in microseconds multiplied by PPQ to retain fractional ticks.
        // A binary search also handles the clock jumping backwards when playback loops.
        let mut segments = vec![(0_u128, 0_u64, 500_000_u32)];
        for (tick, tempo) in tempos {
            let &(time, previous_tick, previous_tempo) = segments.last().unwrap();
            let time = time + u128::from(tick - previous_tick) * u128::from(previous_tempo);
            segments.push((time, tick, tempo));
        }
        let ppq = u128::from(self.ppq);
        move |micros| {
            let target = u128::from(micros) * ppq;
            let index = segments.partition_point(|&(time, _, _)| time <= target) - 1;
            let (time, tick, tempo) = segments[index];
            // Saturate extreme transport times instead of wrapping back into the song.
            (u128::from(tick) + (target - time) / u128::from(tempo)).min(u128::from(Tick::MAX))
                as Tick
        }
    }

    pub fn remove_track(&mut self, index: usize) {
        let mut removed = self.tracks.remove(index);
        if self.tracks.is_empty() {
            self.add_track();
        }
        removed.events.retain(|(_, event)| {
            matches!(
                event,
                Event::Meta(
                    MetaMessage::Tempo(_)
                        | MetaMessage::TimeSignature(..)
                        | MetaMessage::KeySignature(..)
                )
            )
        });
        self.tracks[0].events.extend(removed.events);
    }

    pub fn bpm(&self) -> f64 {
        let tempo = self
            .tracks
            .iter()
            .flat_map(|track| &track.events)
            .filter_map(|(tick, event)| match event {
                Event::Meta(MetaMessage::Tempo(tempo)) if *tick == 0 && tempo.as_int() != 0 => {
                    Some(tempo.as_int())
                }
                _ => None,
            })
            .next_back()
            .unwrap_or(500_000);
        60_000_000.0 / f64::from(tempo)
    }

    pub fn set_tempo(&mut self, bpm: f64) {
        for track in &mut self.tracks {
            track.events.retain(|(tick, event)| {
                *tick != 0 || !matches!(event, Event::Meta(MetaMessage::Tempo(_)))
            });
        }
        self.tracks[0].events.insert(
            0,
            (
                0,
                Event::Meta(MetaMessage::Tempo(u24::from(
                    (60_000_000.0 / bpm).round() as u32
                ))),
            ),
        );
    }

    pub fn open(path: &Path) -> Result<Self> {
        Self::decode(&fs::read(path)?)
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let smf = Smf::parse(bytes)?;
        if smf.header.format == Format::Sequential {
            return Err(invalid(
                "MIDI format 2 stores independent sequences. Open a format 0 or 1 file.",
            )
            .into());
        }
        let Timing::Metrical(ppq) = smf.header.timing else {
            return Err(invalid("SMPTE-timed MIDI files are unsupported.").into());
        };
        if ppq.as_int() == 0 || smf.tracks.is_empty() {
            return Err(invalid("The MIDI file has no usable timing or tracks.").into());
        }
        let mut tracks: Vec<Track> = smf
            .tracks
            .into_iter()
            .map(|events| {
                let mut track = Track::default();
                let mut first_channel = None;
                let mut note_channel = None;
                for event in events {
                    track.end += u64::from(event.delta.as_int());
                    match event.kind {
                        TrackEventKind::Meta(MetaMessage::EndOfTrack) => continue,
                        TrackEventKind::Midi { channel, message } => {
                            first_channel.get_or_insert(channel.as_int());
                            if matches!(message, MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0) {
                                note_channel.get_or_insert(channel.as_int());
                            }
                        }
                        _ => {}
                    }
                    track.events.push((track.end, Event::from_midi(event.kind)));
                }
                track.channel = note_channel.or(first_channel).unwrap_or(0);
                track
            })
            .collect();
        // Format 1 files may put channel initialization on a separate setup track.
        let mut setup = Vec::new();
        for (track_index, track) in tracks.iter().enumerate() {
            let has_notes = track.events.iter().any(|(_, event)| {
                matches!(event, Event::Midi(_, MidiMessage::NoteOn { vel, .. }) if vel.as_int() > 0)
            });
            if has_notes {
                continue;
            }
            for (event_index, (tick, event)) in track.events.iter().enumerate() {
                if let Event::Midi(channel, message) = event {
                    for (index, cc) in INITIAL_SETTINGS.into_iter().enumerate() {
                        if let Some(value) = setting_value(*message, cc) {
                            setup.push(((*tick, track_index, event_index), *channel, index, value));
                        }
                    }
                }
            }
        }
        setup.sort_unstable_by_key(|entry| entry.0);
        for (track_index, track) in tracks.iter_mut().enumerate() {
            let Some((tick, event_index)) = track.first_note() else {
                continue;
            };
            let mut settings = [None; 4];
            for &(position, channel, index, value) in &setup {
                if position >= (tick, track_index, event_index) {
                    break;
                }
                if channel == track.channel {
                    settings[index] = Some(value);
                }
            }
            track.inherited = Some((track.channel, settings));
        }
        Ok(Self {
            transposed: 0,
            ppq: ppq.as_int(),
            tracks,
        })
    }

    fn encode(&self) -> Result<Vec<u8>> {
        if self.tracks.len() > u16::MAX as usize {
            return Err(invalid("Too many MIDI tracks.").into());
        }
        let format = if self.tracks.len() == 1 {
            Format::SingleTrack
        } else {
            Format::Parallel
        };
        let mut smf = Smf::new(Header::new(format, Timing::Metrical(u15::from(self.ppq))));
        for track in &self.tracks {
            let mut events: Vec<_> = track
                .events
                .iter()
                .map(|(tick, event)| (*tick, event.as_midi()))
                .collect();
            events.sort_by_key(|&(tick, _)| tick);
            let end = track.end.max(events.last().map_or(0, |&(tick, _)| tick));
            events.push((end, TrackEventKind::Meta(MetaMessage::EndOfTrack)));
            let mut previous = 0;
            let mut output = Vec::with_capacity(events.len());
            for (tick, kind) in events {
                let delta = tick - previous;
                if delta > MAX_DELTA {
                    return Err(invalid("The gap between MIDI events is too large to save.").into());
                }
                output.push(TrackEvent {
                    delta: u28::from(delta as u32),
                    kind,
                });
                previous = tick;
            }
            smf.tracks.push(output);
        }
        let mut bytes = Vec::new();
        smf.write_std(&mut bytes)?;
        Ok(bytes)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let bytes = self.encode()?;
        // A failed export must leave any existing project file intact.
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        file.persist(path)?;
        Ok(())
    }

    pub fn timeline(&self) -> Timeline {
        let solo = self.tracks.iter().any(|track| track.solo);
        let mut events = Vec::new();
        let mut end = 0;
        for (index, track) in self.tracks.iter().enumerate() {
            let audible = !track.mute && (!solo || track.solo);
            if audible {
                end = end.max(track.end);
            }
            for (tick, event) in &track.events {
                if matches!(event, Event::Meta(MetaMessage::Tempo(_)))
                    || (audible && matches!(event, Event::Midi(..)))
                {
                    events.push((*tick, index, event));
                    end = end.max(*tick);
                }
            }
        }
        events.sort_by_key(|&(tick, _, _)| tick);
        let ppq = u128::from(self.ppq);
        let mut elapsed = 0_u128;
        let mut previous = 0;
        let mut tempo = 500_000;
        let mut packets = Vec::new();
        for (tick, track, event) in events {
            elapsed += u128::from(tick - previous) * u128::from(tempo);
            previous = tick;
            match event {
                Event::Meta(MetaMessage::Tempo(value)) if value.as_int() != 0 => {
                    tempo = value.as_int()
                }
                Event::Midi(channel, message) => {
                    let (status, a, b, len) = match *message {
                        MidiMessage::NoteOff { key, vel } => (0x80, key.as_int(), vel.as_int(), 3),
                        MidiMessage::NoteOn { key, vel } => (0x90, key.as_int(), vel.as_int(), 3),
                        MidiMessage::Aftertouch { key, vel } => {
                            (0xa0, key.as_int(), vel.as_int(), 3)
                        }
                        MidiMessage::Controller { controller, value } => {
                            (0xb0, controller.as_int(), value.as_int(), 3)
                        }
                        MidiMessage::ProgramChange { program } => (0xc0, program.as_int(), 0, 2),
                        MidiMessage::ChannelAftertouch { vel } => (0xd0, vel.as_int(), 0, 2),
                        MidiMessage::PitchBend { bend } => {
                            let value = bend.0.as_int();
                            (0xe0, (value & 127) as u8, (value >> 7) as u8, 3)
                        }
                    };
                    packets.push(Packet {
                        track,
                        micros: (elapsed / ppq).min(u128::from(u64::MAX)) as u64,
                        bytes: [status | channel, a, b],
                        len,
                    });
                }
                _ => {}
            }
        }
        elapsed += u128::from(end - previous) * u128::from(tempo);
        Timeline {
            instrument_tracks:self.tracks.iter().enumerate().filter(|(_,t)|t.events.iter().any(|(_,event)|matches!(event,Event::Midi(_,MidiMessage::NoteOn{vel,..}) if vel.as_int()>0))).map(|(index,_)|index).collect(),
            buses: self.tracks.iter().map(|t| t.bus.clone()).collect(),
            packets,
            duration: (elapsed / ppq).min(u128::from(u64::MAX)) as u64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpose_moves_notes_and_keys_but_preserves_drums_and_rejects_overflow() {
        let mut song = Song::new();
        song.tracks[0].add_note(0, 480, 60, 80);
        song.add_track();
        song.tracks[1].set_channel(9);
        song.tracks[1].add_note(0, 480, 36, 80);
        song.set_key(0, (0, false));
        song.transpose(2).unwrap();
        assert_eq!(song.transposed, 2);
        assert_eq!(song.tracks[0].notes()[0].key, 62);
        assert_eq!(song.tracks[1].notes()[0].key, 36);
        assert_eq!(
            Song::decode(&song.encode().unwrap()).unwrap().key_map(),
            vec![(0, (2, false))]
        );
        assert!(song.transpose(100).is_err());
        assert_eq!(song.transposed, 2);
        assert_eq!(song.tracks[0].notes()[0].key, 62);
    }

    #[test]
    fn overdub_uses_selected_channel_and_keeps_existing_notes() {
        let mut song = Song::demo();
        let channel = song.tracks[1].channel;
        let before = song.tracks[1].notes().len();
        let selected = song.add_recording(250_000, vec![(0, [0x90, 72, 90])], 1_000_000, Some(1));
        assert_eq!(selected, 1);
        assert_eq!(song.tracks.len(), 2);
        let notes = song.tracks[1].notes();
        assert_eq!(notes.len(), before + 1);
        let note = notes.iter().find(|note| note.key == 72).unwrap();
        assert_eq!((note.channel, note.start, note.end), (channel, 960, 1200));
    }

    #[test]
    fn imported_patch_follows_first_note_channel_and_setup_events() {
        let mut song = Song::new();
        song.tracks[0].events.push((
            24,
            Event::Midi(4, MidiMessage::ProgramChange { program: 40.into() }),
        ));
        song.add_track();
        let track = &mut song.tracks[1];
        track.channel = 4;
        track.events.push((
            0,
            Event::Midi(0, MidiMessage::ProgramChange { program: 1.into() }),
        ));
        track.add_note(48, 96, 60, 100);
        track.events.push((
            120,
            Event::Midi(4, MidiMessage::ProgramChange { program: 73.into() }),
        ));
        let mut loaded = Song::decode(&song.encode().unwrap()).unwrap();
        assert_eq!(loaded.tracks[1].channel, 4);
        assert_eq!(loaded.tracks[1].setting(None), 40);
        loaded.tracks[1].set_setting(None, 42);
        let loaded = Song::decode(&loaded.encode().unwrap()).unwrap();
        assert_eq!(loaded.tracks[1].setting(None), 42);
        assert!(loaded.tracks[1].events.iter().any(|(tick,event)| {
            *tick == 120 && matches!(event, Event::Midi(4, MidiMessage::ProgramChange { program }) if program.as_int() == 73)
        }));
    }

    #[test]
    fn imported_delayed_program_before_first_note_is_editable() {
        let mut song = Song::new();
        let track = &mut song.tracks[0];
        track.events.push((
            40,
            Event::Midi(0, MidiMessage::ProgramChange { program: 56.into() }),
        ));
        track.add_note(40, 80, 60, 100);
        let mut song = Song::decode(&song.encode().unwrap()).unwrap();
        assert_eq!(song.tracks[0].setting(None), 56);
        song.tracks[0].set_setting(None, 57);
        assert_eq!(song.tracks[0].setting(None), 57);
    }

    #[test]
    fn seeking_past_end_preserves_zero_remaining_duration() {
        let mut timeline = Song::demo().timeline();
        timeline.seek(u64::MAX);
        assert_eq!(timeline.duration, 0);
        assert!(timeline.packets.iter().all(|p| p.micros == 0));
    }

    #[test]
    fn extreme_time_saturates_instead_of_wrapping() {
        let mut song = Song::new();
        song.ppq = 32767;
        song.tracks[0]
            .events
            .push((0, Event::Meta(MetaMessage::Tempo(1.into()))));
        assert_eq!(song.tempo_clock()(u64::MAX), Tick::MAX);
    }

    #[test]
    fn playhead_follows_tempo_and_loop_rewind() {
        let mut song = Song::new();
        song.tracks[0]
            .events
            .push((480, Event::Meta(MetaMessage::Tempo(1_000_000.into()))));
        let clock = song.tempo_clock();
        assert_eq!(clock(500_000), 480);
        assert_eq!(clock(1_000_000), 720);
        assert_eq!(clock(250_000), 240);
        assert_eq!(clock(0), 0);
    }

    #[test]
    fn multitrack_midi_round_trip() {
        let mut song = Song::demo();
        song.tracks[0]
            .events
            .push((12, Event::SysEx(vec![0x7d, 0xf7])));
        song.tracks[0].end = 3000;
        let bytes = song.encode().unwrap();
        let loaded = Song::decode(&bytes).unwrap();
        assert_eq!(loaded.tracks[0].name(), "Piano");
        assert_eq!(loaded.tracks[0].notes().len(), 9);
        assert_eq!(loaded.tracks[1].setting(None), 32);
        assert_eq!(loaded.tracks[0].end, 3000);
        assert_eq!(loaded.encode().unwrap(), bytes);
    }

    #[test]
    fn tempo_changes_and_muting() {
        let mut song = Song::demo();
        song.tracks[0].mute = true;
        song.tracks[0]
            .events
            .push((480, Event::Meta(MetaMessage::Tempo(1_000_000.into()))));
        let timeline = song.timeline();
        assert!(timeline
            .packets
            .iter()
            .all(|event| event.bytes[0] & 15 == 1));
        assert!(timeline
            .packets
            .iter()
            .any(|event| event.micros == 1_500_000));
        assert_eq!(timeline.duration, 4_375_000);
    }

    #[test]
    fn edits_update_both_note_events() {
        let mut song = Song::new();
        let track = &mut song.tracks[0];
        let original = track.add_note(0, 120, 60, 80);
        let changed = Note {
            start: 120,
            end: 360,
            key: 61,
            velocity: 100,
            ..original
        };
        track.change_note(changed);
        assert_eq!(track.notes(), [changed]);
        track.delete_note(changed);
        assert!(track.notes().is_empty());
    }
}
