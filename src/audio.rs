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
            let lower = name.to_ascii_lowercase();
            let sound_blaster = [
                "sound blaster",
                "soundblaster",
                "sb live",
                "soundfont",
                "audigy",
                "emu10k",
                "awe32",
                "awe64",
            ]
            .iter()
            .any(|term| lower.contains(term));
            Some(Output {
                port,
                name,
                sound_blaster,
            })
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

pub struct Playback {
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

struct Renderer {
    synth: Synthesizer,
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
    (u128::from(micros) * u128::from(rate) / 1_000_000) as u64
}

impl Renderer {
    fn new(font: &Arc<SoundFont>, timeline: Timeline, rate: u32, looping: bool) -> Result<Self> {
        let mut settings = SynthesizerSettings::new(rate as i32);
        settings.block_size = 32;
        let synth = Synthesizer::new(font, &settings)?;
        let release_frame = frames(timeline.duration, rate);
        Ok(Self {
            synth,
            timeline,
            index: 0,
            frame: 0,
            rate,
            release_frame,
            end_frame: release_frame + if looping { 0 } else { u64::from(rate) },
            released: false,
            looping,
        })
    }

    fn render(&mut self, mut left: &mut [f32], mut right: &mut [f32]) {
        while !left.is_empty() {
            if self.looping && self.frame >= self.end_frame {
                self.synth.reset();
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
                let [status, a, b] = packet.bytes.map(i32::from);
                self.synth
                    .process_midi_message(status & 15, status & 0xf0, a, b);
                self.index += 1;
            }
            if !self.looping && !self.released && self.frame >= self.release_frame {
                self.synth.note_off_all(false);
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
            self.synth.render(out_left, out_right);
            self.frame += count as u64;
            left = rest_left;
            right = rest_right;
        }
    }
}

impl Playback {
    pub fn software(font: &Arc<SoundFont>, timeline: Timeline, looping: bool) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("No default audio output is available.")?;
        let config = device.default_output_config()?;
        let mut renderer = Renderer::new(font, timeline, config.sample_rate().0, looping)?;
        let progress = Arc::new(Progress::default());
        let channels = usize::from(config.channels());
        let (error_tx, errors) = std::sync::mpsc::channel();
        let stream_config = config.config();
        macro_rules! make_stream {
            ($sample:ty) => {{
                let state = Arc::clone(&progress);
                device.build_output_stream(
                    &stream_config,
                    move |out: &mut [$sample], _| {
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
                            renderer.frame * 1_000_000 / u64::from(renderer.rate),
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
            cpal::SampleFormat::I16 => make_stream!(i16),
            cpal::SampleFormat::U16 => make_stream!(u16),
            format => return Err(format!("Unsupported audio sample format: {format}").into()),
        };
        stream.play()?;
        Ok(Self {
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
                'play: loop {
                    let start = Instant::now();
                    let wait_until = |micros| {
                        while !cancelled.load(Ordering::Relaxed) {
                            let elapsed = start.elapsed().as_micros() as u64;
                            state.position.store(elapsed, Ordering::Relaxed);
                            if elapsed >= micros {
                                return true;
                            }
                            thread::park_timeout(Duration::from_micros(
                                (micros - elapsed).min(10_000),
                            ));
                        }
                        false
                    };
                    for Packet { micros, bytes, len } in &timeline.packets {
                        if !wait_until(*micros) {
                            break 'play;
                        }
                        active[usize::from(bytes[0] & 15)] = true;
                        if let Err(err) = connection.send(&bytes[..*len]) {
                            let _ = error_tx.send(err.to_string());
                            break 'play;
                        }
                    }
                    if !wait_until(timeline.duration) || !looping {
                        break;
                    }
                    silence(&mut connection, &mut active);
                }
                silence(&mut connection, &mut active);
                state.done.store(true, Ordering::Relaxed);
            })?;
        Ok(Self {
            progress,
            errors,
            backend: Backend::Hardware {
                stop,
                worker: Some(worker),
            },
        })
    }
}

pub fn export_wav(path: &Path, font: &Arc<SoundFont>, timeline: Timeline) -> Result<()> {
    let rate = 44_100;
    let mut renderer = Renderer::new(font, timeline, rate, false)?;
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
