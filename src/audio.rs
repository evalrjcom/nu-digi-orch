use crate::song::{Packet, Result, Timeline};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midir::{MidiOutput, MidiOutputPort};
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::{
    fs::File,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub struct Output {
    pub port: MidiOutputPort,
    pub name: String,
    pub sound_blaster: bool,
}

pub fn outputs() -> Result<Vec<Output>> {
    let midi = MidiOutput::new("Nu Digi Orch")?;
    Ok(midi
        .ports()
        .into_iter()
        .filter_map(|port| {
            let name = midi.port_name(&port).ok()?;
            let sound_blaster = is_sound_blaster(&name);
            Some(Output {
                port,
                name,
                sound_blaster,
            })
        })
        .collect())
}

pub fn is_sound_blaster(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "sound blaster",
        "soundblaster",
        "sb live",
        "sblive",
        "soundfont",
        "audigy",
        "emu10k",
        "awe32",
        "awe64",
        "ca0106",
        "creative technology",
    ]
    .iter()
    .any(|term| name.contains(term))
}

pub struct SoundDevice {
    pub device: cpal::Device,
    pub name: String,
}

pub fn sound_devices() -> Result<Vec<SoundDevice>> {
    Ok(cpal::default_host()
        .output_devices()?
        .filter_map(|device| {
            let name = device.name().ok()?;
            Some(SoundDevice { device, name })
        })
        .collect())
}

pub fn load_font(path: &Path) -> Result<Arc<SoundFont>> {
    Ok(Arc::new(SoundFont::new(&mut File::open(path)?)?))
}

#[derive(Default)]
pub struct Progress {
    pub position: AtomicU64,
    pub done: AtomicBool,
    pub paused: AtomicBool,
}

enum Backend {
    Software {
        _stream: cpal::Stream,
    },
    Hardware {
        stop: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    },
}

pub struct PlaybackEdit {
    pub timeline: Timeline,
    pub messages: Vec<Packet>,
    // Routing or tempo changes need a fresh note/controller state at the same musical tick.
    pub reposition: Option<u64>,
}

pub struct Playback {
    pub edits: std::sync::mpsc::Sender<PlaybackEdit>,
    pub progress: Arc<Progress>,
    pub errors: std::sync::mpsc::Receiver<String>,
    backend: Backend,
}

impl Drop for Playback {
    fn drop(&mut self) {
        if let Backend::Hardware { stop, worker } = &mut self.backend {
            stop.store(true, Ordering::Relaxed);
            if let Some(worker) = worker.take() {
                worker.thread().unpark();
                let _ = worker.join();
            }
        }
    }
}

struct Stem {
    synth: Synthesizer,
    filter: crate::bus::Filter,
    track: usize,
}
struct Renderer {
    rack: crate::effects::Rack,
    levels: Arc<crate::effects::Levels>,
    stems: Vec<Stem>,
    master: crate::bus::Filter,
    timeline: Timeline,
    index: usize,
    frame: u64,
    rate: u32,
    release_frame: u64,
    end_frame: u64,
    released: bool,
    looping: bool,
}

fn frames(micros: u64, rate: u32) -> u64 {
    (u128::from(micros) * u128::from(rate) / 1_000_000).min(u128::from(u64::MAX)) as u64
}

// Playback and export share the release window so wet tails sound the same.
fn render_end(release: u64, rate: u32, looping: bool, wet: bool) -> u64 {
    release.saturating_add(if looping {
        0
    } else {
        u64::from(rate) * if wet { 4 } else { 1 }
    })
}

impl Renderer {
    fn new(
        font: &Arc<SoundFont>,
        timeline: Timeline,
        rate: u32,
        looping: bool,
        levels: Arc<crate::effects::Levels>,
    ) -> Result<Self> {
        let mut settings = SynthesizerSettings::new(rate as i32);
        settings.block_size = 32;
        let mut stems = Vec::new();
        for &track in &timeline.instrument_tracks {
            {
                stems.push(Stem {
                    synth: Synthesizer::new(font, &settings)?,
                    filter: crate::bus::Filter::new(rate),
                    track,
                });
            }
        }
        // A sub-sample loop still needs one frame to make progress in the callback.
        let release_frame = frames(timeline.duration, rate).max(1);
        let end_frame = render_end(release_frame, rate, looping, levels.get() != [0; 3]);
        Ok(Self {
            rack: crate::effects::Rack::new(rate),
            levels,
            stems,
            master: crate::bus::Filter::new(rate),
            timeline,
            index: 0,
            frame: 0,
            rate,
            release_frame,
            end_frame,
            released: false,
            looping,
        })
    }

    fn dispatch(stems: &mut [Stem], p: Packet) {
        let [status, a, b] = p.bytes.map(i32::from);
        let kind = status & 0xf0;
        let target = stems.binary_search_by_key(&p.track, |stem| stem.track).ok();
        // Notes and mixer parameters on an instrument track belong to its stem.
        // Controller-only tracks still provide shared channel settings for imported MIDI.
        let local =
            matches!(kind, 0x80 | 0x90 | 0xc0) || (kind == 0xb0 && matches!(a, 0 | 7 | 10 | 32));
        if let Some(index) = target.filter(|_| local) {
            stems[index]
                .synth
                .process_midi_message(status & 15, kind, a, b);
        } else if !matches!(kind, 0x80 | 0x90) {
            for stem in stems {
                stem.synth.process_midi_message(status & 15, kind, a, b);
            }
        }
    }

    fn render(&mut self, mut left: &mut [f32], mut right: &mut [f32]) {
        // Capture live effect changes while the song is still playing.
        if !self.released {
            self.end_frame = render_end(
                self.release_frame,
                self.rate,
                self.looping,
                self.levels.get() != [0; 3],
            );
        }
        while !left.is_empty() {
            if self.looping && self.frame >= self.end_frame {
                for stem in &mut self.stems {
                    stem.synth.reset();
                }
                self.frame = 0;
                self.index = 0;
            } else if self.frame >= self.end_frame {
                left.fill(0.0);
                right.fill(0.0);
                return;
            }

            while let Some(packet) = self.timeline.packets.get(self.index) {
                if frames(packet.micros, self.rate) > self.frame {
                    break;
                }
                Self::dispatch(&mut self.stems, *packet);
                self.index += 1;
            }
            if !self.looping && !self.released && self.frame >= self.release_frame {
                for stem in &mut self.stems {
                    // Clear held pedals before releasing voices at the end of the arrangement.
                    for channel in 0..16 {
                        for controller in [64, 66] {
                            stem.synth
                                .process_midi_message(channel, 0xb0, controller, 0);
                        }
                    }
                    stem.synth.note_off_all(false);
                }
                self.released = true;
            }

            let next_event = self
                .timeline
                .packets
                .get(self.index)
                .map_or(self.end_frame, |packet| frames(packet.micros, self.rate));
            let boundary = if self.released || self.looping {
                self.end_frame
            } else {
                self.release_frame
            };
            let count = (boundary.min(next_event) - self.frame).min(left.len() as u64) as usize;
            let (out_left, rest_left) = left.split_at_mut(count);
            let (out_right, rest_right) = right.split_at_mut(count);
            out_left.fill(0.);
            out_right.fill(0.);
            for stem in &mut self.stems {
                for (l, r) in out_left.chunks_mut(256).zip(out_right.chunks_mut(256)) {
                    let mut a = [0.; 256];
                    let mut b = [0.; 256];
                    let n = l.len();
                    stem.synth.render(&mut a[..n], &mut b[..n]);
                    stem.filter
                        .process(&mut a[..n], &mut b[..n], &self.timeline.buses[stem.track]);
                    for i in 0..n {
                        l[i] += a[i];
                        r[i] += b[i];
                    }
                }
            }
            self.rack.process(out_left, out_right, self.levels.get());
            self.master.process(out_left, out_right, &self.levels.1);
            self.frame += count as u64;
            left = rest_left;
            right = rest_right;
        }
    }
}

impl Playback {
    pub fn is_software(&self) -> bool {
        matches!(self.backend, Backend::Software { .. })
    }

    pub fn software(
        font: &Arc<SoundFont>,
        timeline: Timeline,
        looping: bool,
        output: Option<&SoundDevice>,
        levels: Arc<crate::effects::Levels>,
    ) -> Result<Self> {
        let default;
        let device = match output {
            Some(output) => &output.device,
            None => {
                default = cpal::default_host()
                    .default_output_device()
                    .ok_or("No default audio output is available.")?;
                &default
            }
        };
        let config = device.default_output_config()?;
        let mut renderer = Renderer::new(font, timeline, config.sample_rate().0, looping, levels)?;
        let progress = Arc::new(Progress::default());
        let channels = usize::from(config.channels());
        let (error_tx, errors) = std::sync::mpsc::channel();
        let (edits, updates) = std::sync::mpsc::channel::<PlaybackEdit>();
        let stream_config = config.config();
        macro_rules! make_stream {
            ($sample:ty) => {{
                let state = Arc::clone(&progress);
                device.build_output_stream(
                    &stream_config,
                    move |out: &mut [$sample], _| {
                        if state.paused.load(Ordering::Acquire) {
                            // Keep the synth, effect history, and sample position frozen.
                            out.fill(<$sample as cpal::Sample>::from_sample(0.0_f32));
                            return;
                        }
                        for update in updates.try_iter() {
                            if let Some(micros) = update.reposition {
                                for stem in &mut renderer.stems {
                                    stem.synth.reset();
                                }
                                renderer.frame = frames(micros, renderer.rate);
                                renderer.released = false;
                            }
                            for packet in update.messages {
                                Renderer::dispatch(&mut renderer.stems, packet);
                            }
                            renderer.timeline = update.timeline;
                            renderer.index = renderer.timeline.packets.partition_point(|p| {
                                frames(p.micros, renderer.rate) < renderer.frame
                            });
                            renderer.release_frame =
                                frames(renderer.timeline.duration, renderer.rate).max(1);
                            renderer.end_frame = render_end(
                                renderer.release_frame,
                                renderer.rate,
                                renderer.looping,
                                renderer.levels.get() != [0; 3],
                            );
                        }
                        let mut left = [0.0; 256];
                        let mut right = [0.0; 256];
                        for chunk in out.chunks_mut(channels * 256) {
                            let count = chunk.len() / channels;
                            renderer.render(&mut left[..count], &mut right[..count]);
                            for (index, frame) in chunk.chunks_exact_mut(channels).enumerate() {
                                for (channel, sample) in frame.iter_mut().enumerate() {
                                    let value = match (channels, channel) {
                                        (1, _) => (left[index] + right[index]) * 0.5,
                                        (_, 0) => left[index],
                                        (_, 1) => right[index],
                                        _ => 0.0,
                                    };
                                    *sample = <$sample as cpal::Sample>::from_sample(
                                        value.clamp(-1.0, 1.0),
                                    );
                                }
                            }
                        }
                        state.position.store(
                            (u128::from(renderer.frame) * 1_000_000 / u128::from(renderer.rate))
                                as u64,
                            Ordering::Relaxed,
                        );
                        state.done.store(
                            !renderer.looping && renderer.frame >= renderer.end_frame,
                            Ordering::Relaxed,
                        );
                    },
                    move |err| {
                        let _ = error_tx.send(err.to_string());
                    },
                    None,
                )?
            }};
        }
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => make_stream!(f32),
            cpal::SampleFormat::F64 => make_stream!(f64),
            cpal::SampleFormat::I8 => make_stream!(i8),
            cpal::SampleFormat::I32 => make_stream!(i32),
            cpal::SampleFormat::I64 => make_stream!(i64),
            cpal::SampleFormat::U8 => make_stream!(u8),
            cpal::SampleFormat::U32 => make_stream!(u32),
            cpal::SampleFormat::U64 => make_stream!(u64),
            cpal::SampleFormat::I16 => make_stream!(i16),
            cpal::SampleFormat::U16 => make_stream!(u16),
            format => return Err(format!("Unsupported audio sample format: {format}").into()),
        };
        stream.play()?;
        Ok(Self {
            edits,
            progress,
            errors,
            backend: Backend::Software { _stream: stream },
        })
    }

    pub fn hardware(port: MidiOutputPort, timeline: Timeline, looping: bool) -> Result<Self> {
        let mut connection = MidiOutput::new("Nu Digi Orch")?
            .connect(&port, "Composer output")
            .map_err(|err| err.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&stop);
        let progress = Arc::new(Progress::default());
        let state = Arc::clone(&progress);
        let (error_tx, errors) = std::sync::mpsc::channel();
        let (edits, updates) = std::sync::mpsc::channel::<PlaybackEdit>();
        let worker = thread::Builder::new()
            .name("midi-output".into())
            .spawn(move || {
                let mut active = [false; 16];
                let silence = |connection: &mut midir::MidiOutputConnection,
                               active: &mut [bool; 16]| {
                    for (channel, used) in active.iter_mut().enumerate().filter(|(_, used)| **used)
                    {
                        for (cc, value) in [(64, 0), (66, 0), (120, 0), (123, 0)] {
                            let _ = connection.send(&[0xb0 | channel as u8, cc, value]);
                        }
                        *used = false;
                    }
                };
                let mut timeline = timeline;
                let mut origin = Instant::now();
                let mut offset = 0;
                let mut index = 0;
                'play: while !cancelled.load(Ordering::Relaxed) {
                    let mut elapsed = offset + origin.elapsed().as_micros() as u64;
                    for update in updates.try_iter() {
                        if let Some(position) = update.reposition {
                            silence(&mut connection, &mut active);
                            offset = position;
                            origin = Instant::now();
                            elapsed = position;
                        }
                        for packet in update.messages {
                            active[usize::from(packet.bytes[0] & 15)] = true;
                            if let Err(err) = connection.send(&packet.bytes[..packet.len]) {
                                let _ = error_tx.send(err.to_string());
                                break 'play;
                            }
                        }
                        // Preserve pending events that became due between worker polls.
                        // Only an explicit reposition may discard that interval.
                        let boundary = if update.reposition.is_some() {
                            elapsed
                        } else {
                            timeline.packets.get(index).map_or(u64::MAX, |p| p.micros)
                        };
                        timeline = update.timeline;
                        index = timeline.packets.partition_point(|p| p.micros < boundary);
                    }
                    state.position.store(elapsed, Ordering::Relaxed);
                    while let Some(p) = timeline.packets.get(index).filter(|p| p.micros <= elapsed)
                    {
                        active[usize::from(p.bytes[0] & 15)] = true;
                        if let Err(err) = connection.send(&p.bytes[..p.len]) {
                            let _ = error_tx.send(err.to_string());
                            break 'play;
                        }
                        index += 1;
                    }
                    if elapsed >= timeline.duration {
                        if !looping {
                            break;
                        }
                        silence(&mut connection, &mut active);
                        index = 0;
                        offset = 0;
                        origin = Instant::now();
                        continue;
                    }
                    let next = timeline
                        .packets
                        .get(index)
                        .map_or(timeline.duration, |p| p.micros);
                    thread::park_timeout(Duration::from_micros(
                        next.saturating_sub(elapsed).min(2000),
                    ));
                }
                silence(&mut connection, &mut active);
                state.done.store(true, Ordering::Relaxed);
            })?;
        Ok(Self {
            edits,
            progress,
            errors,
            backend: Backend::Hardware {
                stop,
                worker: Some(worker),
            },
        })
    }
}

pub fn export_wav(
    path: &Path,
    font: &Arc<SoundFont>,
    timeline: Timeline,
    levels: Arc<crate::effects::Levels>,
) -> Result<()> {
    let rate = 44_100;
    let mut renderer = Renderer::new(font, timeline, rate, false, levels)?;
    if renderer.end_frame > u64::from((u32::MAX - 64) / 4) {
        return Err("The arrangement is too long for a stereo 16-bit WAV file.".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let mut output = hound::WavWriter::new(
        std::io::BufWriter::new(temp.as_file_mut()),
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    while renderer.frame < renderer.end_frame {
        let count = (renderer.end_frame - renderer.frame).min(left.len() as u64) as usize;
        renderer.render(&mut left[..count], &mut right[..count]);
        for (&l, &r) in left[..count].iter().zip(&right) {
            output.write_sample((l.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)?;
            output.write_sample((r.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16)?;
        }
    }
    output.finalize()?;
    temp.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod mix_tests {
    use super::*;
    use crate::{effects::Levels, song::Song};

    #[test]
    fn tail_matches_playback_and_export() {
        assert_eq!(render_end(44_100, 44_100, false, false), 88_200);
        assert_eq!(render_end(44_100, 44_100, false, true), 220_500);
        assert_eq!(render_end(44_100, 44_100, true, true), 44_100);
        assert_eq!(render_end(u64::MAX, 44_100, false, true), u64::MAX);
    }

    #[test]
    #[ignore = "requires an SF2 path in NU_DIGI_ORCH_SF2"]
    fn soundfont_stems_and_master_process_real_audio() {
        let font = load_font(Path::new(&std::env::var("NU_DIGI_ORCH_SF2").unwrap())).unwrap();
        let mut song = Song::demo();
        song.tracks[1].set_channel(0);
        let levels = Arc::new(Levels::default());
        let render = |song: &Song| {
            let mut engine =
                Renderer::new(&font, song.timeline(), 44100, false, levels.clone()).unwrap();
            let mut left = vec![0.; 44100];
            let mut right = left.clone();
            engine.render(&mut left, &mut right);
            left
        };
        let dry = render(&song);
        assert!(song.tracks.iter().all(|track| track.bus.rms() > 0.));
        assert!(levels.1.rms() > 0.);
        song.tracks[1].bus.set(10, -6, 100);
        let shaped = render(&song);
        assert!(dry.iter().zip(shaped).any(|(a, b)| (a - b).abs() > 0.001));
        let piano_level = song.tracks[0].bus.rms();
        song.tracks[1].set_setting(Some(7), 0);
        render(&song);
        assert_eq!(song.tracks[0].bus.rms(), piano_level);
        levels.1.set(0, 0, 0);
        assert!(render(&song).iter().all(|&sample| sample == 0.));
    }
}
