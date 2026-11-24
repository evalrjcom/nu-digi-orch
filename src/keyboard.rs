use super::*;
use std::sync::mpsc::Sender;

const WHITE: [u8; 15] = [0, 2, 4, 5, 7, 9, 11, 12, 14, 16, 17, 19, 21, 23, 24];
const BLACK: [(usize, u8); 10] = [
    (0, 1),
    (1, 3),
    (3, 6),
    (4, 8),
    (5, 10),
    (7, 13),
    (8, 15),
    (10, 18),
    (11, 20),
    (12, 22),
];
const KEYS: &str = "zsxdcvgbhnjmq2w3er5t6y7ui";

struct State {
    base: u8,
    held: [bool; 25],
    mouse: Option<usize>,
    channel: u8,
    sender: Option<Sender<song::Packet>>,
    stream: Option<audio::Playback>,
}

impl State {
    fn change(&mut self, index: usize, down: bool, mouse: bool) {
        let before = self.held[index] || self.mouse == Some(index);
        if mouse {
            self.mouse = down.then_some(index);
        } else {
            self.held[index] = down;
        }
        let after = self.held[index] || self.mouse == Some(index);
        if before != after {
            if let Some(sender) = &self.sender {
                let _ = sender.send(packet(
                    self.channel,
                    if after { 0x90 } else { 0x80 },
                    self.base + index as u8,
                    if after { 96 } else { 0 },
                    3,
                ));
            }
        }
    }

    fn release(&mut self) {
        if let Some(index) = self.mouse {
            self.change(index, false, true);
        }
        for index in 0..self.held.len() {
            self.change(index, false, false);
        }
    }
}

fn packet(channel: u8, code: u8, a: u8, b: u8, len: usize) -> song::Packet {
    song::Packet {
        track: 0,
        micros: 0,
        bytes: [code | channel, a, b],
        len,
    }
}

pub(super) struct Keyboard {
    window: Window,
    keys: Frame,
    state: Rc<RefCell<State>>,
}

impl Keyboard {
    pub fn new() -> Self {
        let mut window = Window::new(140, 160, 720, 222, "Virtual piano");
        window.set_color(BG);
        flat_label(
            14,
            8,
            690,
            28,
            "Z–M and Q–I play notes    •    ← / → change octave",
            14,
            TEXT,
        );
        let mut keys = Frame::new(14, 44, 692, 159, None);
        let state = Rc::new(RefCell::new(State {
            base: 48,
            held: [false; 25],
            mouse: None,
            channel: 0,
            sender: None,
            stream: None,
        }));
        let shared = state.clone();
        keys.draw(move |f| {
            let s = shared.borrow();
            let width = f.w() / WHITE.len() as i32;
            let active = |note: u8| s.held[note as usize] || s.mouse == Some(note as usize);
            draw::set_font(Font::Helvetica, 13);
            for (i, note) in WHITE.into_iter().enumerate() {
                let x = f.x() + i as i32 * width;
                draw::draw_rect_fill(
                    x,
                    f.y(),
                    width - 1,
                    f.h(),
                    if active(note) { ACCENT } else { Color::White },
                );
                draw::set_draw_color(BG);
                let key = KEYS.as_bytes()[note as usize] as char;
                let label = if note % 12 == 0 {
                    format!("{key} · C{}", (s.base + note) / 12 - 1)
                } else {
                    key.to_string()
                };
                draw::draw_text2(&label, x, f.y() + f.h() - 36, width - 1, 30, Align::Center);
            }
            for (i, note) in BLACK {
                let x = f.x() + (i as i32 + 1) * width - width / 3;
                let h = f.h() * 3 / 5;
                draw::draw_rect_fill(
                    x,
                    f.y(),
                    width * 2 / 3,
                    h,
                    if active(note) { ACCENT } else { BG },
                );
                draw::set_draw_color(if active(note) { BG } else { TEXT });
                draw::draw_text2(
                    &(KEYS.as_bytes()[note as usize] as char).to_string(),
                    x,
                    f.y() + h - 26,
                    width * 2 / 3,
                    22,
                    Align::Center,
                );
            }
        });
        let shared = state.clone();
        keys.handle(move |frame, event| {
            let mut s = shared.borrow_mut();
            match event {
                Event::Focus => return true,
                Event::Unfocus => s.release(),
                Event::Push | Event::Drag => {
                    if event == Event::Push && app::event_mouse_button() != MouseButton::Left {
                        return false;
                    }
                    let x = app::event_x() - frame.x();
                    let y = app::event_y() - frame.y();
                    let w = frame.w() / WHITE.len() as i32;
                    let mut note = None;
                    if (0..w * WHITE.len() as i32).contains(&x) && (0..frame.h()).contains(&y) {
                        note = Some(WHITE[(x / w) as usize] as usize);
                        if y < frame.h() * 3 / 5 {
                            if let Some((_, black)) = BLACK.into_iter().find(|(i, _)| {
                                let edge = (*i as i32 + 1) * w - w / 3;
                                (edge..edge + w * 2 / 3).contains(&x)
                            }) {
                                note = Some(black as usize);
                            }
                        }
                    }
                    if s.mouse != note {
                        if let Some(old) = s.mouse {
                            s.change(old, false, true);
                        }
                        if let Some(note) = note {
                            s.change(note, true, true);
                        }
                    }
                }
                Event::Released => {
                    if let Some(note) = s.mouse {
                        s.change(note, false, true);
                    }
                }
                _ => return false,
            }
            frame.redraw();
            true
        });
        let shared = state.clone();
        let mut display = keys.clone();
        window.handle(move |_, event| {
            let mut s = shared.borrow_mut();
            match event {
                Event::Unfocus => s.release(),
                Event::KeyDown | Event::KeyUp => {
                    let down = event == Event::KeyDown;
                    let key = app::event_key();
                    if down && matches!(key, Key::Left | Key::Right) {
                        s.release();
                        s.base = if key == Key::Left {
                            s.base.saturating_sub(12)
                        } else {
                            (s.base + 12).min(96)
                        };
                    } else if let Some(index) =
                        KEYS.bytes().position(|c| key == Key::from_char(c as char))
                    {
                        if down
                            && app::event_state()
                                .intersects(Shortcut::Ctrl | Shortcut::Alt | Shortcut::Meta)
                        {
                            return false;
                        }
                        s.change(index, down, false);
                    } else {
                        return false;
                    }
                }
                _ => return false,
            }
            display.redraw();
            true
        });
        let shared = state.clone();
        window.set_callback(move |w| {
            let mut s = shared.borrow_mut();
            s.release();
            s.sender = None;
            s.stream = None;
            w.hide();
        });
        window.end();
        Self {
            window,
            keys,
            state,
        }
    }

    pub fn open(
        &mut self,
        font: &Arc<rustysynth::SoundFont>,
        track: &song::Track,
        levels: Arc<effects::Levels>,
        device: Option<&audio::SoundDevice>,
    ) -> song::Result<()> {
        if !self.window.shown() {
            let channel = track.channel;
            let timeline = song::Timeline {
                instrument_tracks: vec![0],
                buses: vec![Arc::new(track.bus.snapshot())],
                packets: vec![],
                duration: 86_400_000_000,
            };
            // Keep audition voices separate from the song's transport and export.
            let stream = audio::Playback::software(font, timeline, true, device, levels)?;
            let sender = stream
                .notes
                .as_ref()
                .expect("software audition input")
                .clone();
            for (cc, value) in [
                (0, track.setting(Some(0))),
                (7, track.setting(Some(7))),
                (10, track.setting(Some(10))),
            ] {
                sender.send(packet(channel, 0xb0, cc, value, 3))?;
            }
            sender.send(packet(channel, 0xc0, track.setting(None), 0, 2))?;
            let mut state = self.state.borrow_mut();
            state.channel = channel;
            state.sender = Some(sender);
            state.stream = Some(stream);
        }
        self.window.show();
        self.keys.redraw();
        Ok(())
    }
}
