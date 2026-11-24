#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod recording;
mod song;

use fltk::{
    app::{self, MouseButton},
    browser::HoldBrowser,
    button::{Button, CheckButton},
    dialog, draw,
    enums::{Align, Color, Event, Font, FrameType, Key, Shortcut},
    frame::Frame,
    input::{Input, IntInput},
    menu::{Choice, MenuBar, MenuFlag},
    prelude::*,
    window::Window,
};
use song::{Note, Song, Tick};
use std::{
    cell::RefCell,
    ffi::OsString,
    path::Path,
    path::PathBuf,
    rc::Rc,
    sync::{atomic::Ordering, Arc},
};

const WIDTH: i32 = 1180;
const HEIGHT: i32 = 730;
const LABEL: i32 = 47;
const RULER: i32 = 26;
const ROWS: i32 = 24;
const BEATS: u64 = 16;
const BG: Color = Color::from_rgb(20, 25, 36);
const PANEL: Color = Color::from_rgb(27, 34, 49);
const TEXT: Color = Color::from_rgb(224, 232, 243);
const SUBTLE: Color = Color::from_rgb(156, 174, 198);
const ACCENT: Color = Color::from_rgb(64, 211, 181);

#[derive(Clone, Copy)]
struct GridPoint {
    tick: Tick,
    key: u8,
}
#[derive(Clone, Copy)]
enum Action {
    New,
    Demo,
    Open,
    Save,
    SaveAs,
    Export,
    Quit,
    Undo,
    Redo,
    AddTrack,
    RemoveTrack,
    SelectTrack,
    TrackSettings,
    Preset,
    Tempo,
    View,
    Play,
    Record,
    Stop,
    OutputChanged,
    LoadFont,
    Refresh,
    ApplyNote,
    DeleteNote,
    SnapNote,
    Grid(GridPoint, bool),
    MoveNote(GridPoint, GridPoint),
}

struct Model {
    song: Song,
    track: usize,
    notes: Vec<Note>,
    selected: Option<Note>,
    undo: Vec<Song>,
    redo: Vec<Song>,
    path: Option<PathBuf>,
    dirty: bool,
    font: Option<Arc<rustysynth::SoundFont>>,
    outputs: Vec<audio::Output>,
    sound_devices: Vec<audio::SoundDevice>,
    playback: Option<audio::Playback>,
    recording: Option<recording::Recording>,
    captured: Vec<(u64, [u8; 3])>,
    export_in_progress: bool,
    inputs: Vec<recording::InputPort>,
    view_beat: Tick,
    high_key: u8,
    step: Tick,
}

impl Model {
    fn new() -> Self {
        let song = Song::new();
        Self {
            step: u64::from(song.ppq) / 4,
            song,
            track: 0,
            notes: vec![],
            selected: None,
            undo: vec![],
            redo: vec![],
            path: None,
            dirty: false,
            font: None,
            outputs: vec![],
            sound_devices: Vec::new(),
            playback: None,
            recording: None,
            captured: Vec::new(),
            export_in_progress: false,
            inputs: Vec::new(),
            view_beat: 0,
            high_key: 83,
        }
    }
    fn checkpoint(&mut self) {
        self.playback = None;
        if self.undo.len() == 32 {
            self.undo.remove(0);
        }
        self.undo.push(self.song.clone());
        self.redo.clear();
        self.dirty = true;
    }
    fn history(&mut self, redo: bool) {
        let (from, into) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(song) = from.pop() {
            self.playback = None;
            into.push(std::mem::replace(&mut self.song, song));
            self.track = self.track.min(self.song.tracks.len() - 1);
            self.selected = None;
            self.dirty = true;
        }
    }
    fn replace(&mut self, song: Song, path: Option<PathBuf>, dirty: bool) {
        self.playback = None;
        self.song = song;
        self.path = path;
        self.dirty = dirty;
        self.track = 0;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.view_beat = 0;
        self.high_key = 83;
        self.fit_pitch();
    }
    fn fit_pitch(&mut self) {
        let notes = self.song.tracks[self.track].notes();
        if let (Some(lowest), Some(highest)) = (
            notes.iter().map(|note| note.key).min(),
            notes.iter().map(|note| note.key).max(),
        ) {
            let midpoint = (u16::from(lowest) + u16::from(highest)) / 2;
            self.high_key = (midpoint + ROWS as u16 / 2).clamp(ROWS as u16 - 1, 127) as u8;
        }
    }

    fn hit(&self, point: GridPoint) -> Option<Note> {
        self.notes.iter().rev().copied().find(|n| {
            n.key == point.key && point.tick >= n.start && point.tick < n.end.max(n.start + 1)
        })
    }
}

fn flat_label(x: i32, y: i32, w: i32, h: i32, label: &str, size: i32, color: Color) -> Frame {
    let mut frame = Frame::new(x, y, w, h, label);
    frame.set_align(Align::Inside | Align::Left);
    frame.set_label_size(size);
    frame.set_label_color(color);
    frame
}

fn button(x: i32, y: i32, w: i32, label: &str, tx: app::Sender<Action>, action: Action) -> Button {
    let mut button = Button::new(x, y, w, 29, label);
    button.emit(tx, action);
    button
}

fn number(x: i32, y: i32, w: i32, value: i32) -> IntInput {
    let mut field = IntInput::new(x, y, w, 26, None);
    field.set_value(&value.to_string());
    field
}

fn menu_item(
    menu: &mut MenuBar,
    path: &str,
    key: Option<char>,
    tx: app::Sender<Action>,
    action: Action,
) {
    menu.add(
        path,
        key.map_or(Shortcut::None, |ch| Shortcut::Ctrl | ch),
        MenuFlag::Normal,
        move |_| tx.send(action),
    );
}

struct Ui {
    win: Window,
    tracks: HoldBrowser,
    name: Input,
    channel: IntInput,
    patch: IntInput,
    bank: IntInput,
    preset: Choice,
    presets: Vec<(i32, i32)>,
    volume: IntInput,
    pan: IntInput,
    mute: CheckButton,
    solo: CheckButton,
    bpm: Input,
    output: Choice,
    audio_device: Choice,
    midi_input: Choice,
    font: Frame,
    looping: CheckButton,
    beat: IntInput,
    pitch: IntInput,
    snap: Choice,
    length: Choice,
    start: IntInput,
    key: IntInput,
    duration: IntInput,
    velocity: IntInput,
    grid: Frame,
    status: Frame,
    clock: Frame,
}

impl Ui {
    fn new(tx: app::Sender<Action>, model: &Rc<RefCell<Model>>) -> Self {
        app::set_scheme(app::Scheme::Gtk);
        app::background(36, 44, 60);
        app::background2(25, 32, 47);
        app::foreground(229, 236, 246);
        let mut win = Window::new(80, 55, WIDTH, HEIGHT, "Nu Digi Orch");
        win.set_color(BG);
        win.size_range(1000, 690, 0, 0);

        let mut menu = MenuBar::new(0, 0, WIDTH, 30, None);
        menu.set_color(PANEL);
        for (path, key, action) in [
            ("File/New", Some('n'), Action::New),
            ("File/Open MIDI…", Some('o'), Action::Open),
            ("File/Save MIDI", Some('s'), Action::Save),
            ("File/Save MIDI As…", None, Action::SaveAs),
            ("File/Export WAV…", None, Action::Export),
            ("File/Open Demo", None, Action::Demo),
            ("File/Quit", Some('q'), Action::Quit),
            ("Edit/Undo", Some('z'), Action::Undo),
            ("Edit/Redo", Some('y'), Action::Redo),
            ("Edit/Delete Note", None, Action::DeleteNote),
            ("Edit/Quantize Selected Note", None, Action::SnapNote),
            ("Track/Add Track", None, Action::AddTrack),
            ("Track/Remove Track", None, Action::RemoveTrack),
            ("Playback/Load SoundFont…", None, Action::LoadFont),
            ("Playback/Refresh Devices", None, Action::Refresh),
        ] {
            menu_item(&mut menu, path, key, tx, action);
        }

        let mut play = button(16, 43, 66, "Play", tx, Action::Play);
        play.set_color(ACCENT);
        play.set_label_color(BG);
        button(88, 43, 64, "Stop", tx, Action::Stop);
        let looping = CheckButton::new(166, 44, 67, 28, "Loop");
        flat_label(239, 43, 40, 29, "BPM", 13, SUBTLE);
        let mut bpm = Input::new(277, 44, 61, 26, None);
        bpm.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
        bpm.emit(tx, Action::Tempo);
        button(345, 43, 48, "Set", tx, Action::Tempo);
        flat_label(414, 43, 53, 29, "Output", 13, SUBTLE);
        let mut output = Choice::new(468, 43, 241, 29, None);
        output.add_choice("Software SoundFont");
        output.set_value(0);
        output.emit(tx, Action::OutputChanged);
        button(718, 43, 114, "Load SF2…", tx, Action::LoadFont);
        let font = flat_label(844, 43, 174, 29, "Choose an SF2 file", 12, SUBTLE);
        let clock = flat_label(1030, 43, 115, 29, "00:00.0", 17, ACCENT);

        flat_label(16, 83, 100, 27, "Tracks", 16, TEXT);
        button(125, 83, 49, "+", tx, Action::AddTrack);
        button(181, 83, 50, "−", tx, Action::RemoveTrack);
        let mut tracks = HoldBrowser::new(16, 120, 215, 222, None);
        tracks.set_color(PANEL);
        tracks.emit(tx, Action::SelectTrack);
        flat_label(16, 354, 195, 25, "Track settings", 16, TEXT);
        let mut name = Input::new(16, 385, 215, 26, None);
        name.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
        name.emit(tx, Action::TrackSettings);
        let mut fields = Vec::new();
        for (row, label) in [
            "MIDI channel (1–16)",
            "Program (0–127)",
            "Bank (CC0)",
            "Volume (CC7)",
            "Pan (CC10)",
        ]
        .into_iter()
        .enumerate()
        {
            let y = 420 + row as i32 * 29;
            flat_label(17, y, 151, 25, label, 13, SUBTLE);
            let mut field = number(172, y, 59, 0);
            field.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
            field.emit(tx, Action::TrackSettings);
            fields.push(field);
        }
        let [channel, patch, bank, volume, pan] =
            fields.try_into().unwrap_or_else(|_| unreachable!());
        let mut mute = CheckButton::new(16, 567, 90, 29, "Mute");
        let mut solo = CheckButton::new(112, 567, 90, 29, "Solo");
        mute.emit(tx, Action::TrackSettings);
        solo.emit(tx, Action::TrackSettings);
        button(
            16,
            601,
            215,
            "Apply track settings",
            tx,
            Action::TrackSettings,
        );

        let mut preset = Choice::new(16, 640, 215, 29, None);
        preset.add_choice("SoundFont presets…");
        preset.set_value(0);
        preset.emit(tx, Action::Preset);
        flat_label(249, 85, 81, 25, "First beat", 13, SUBTLE);
        let mut beat = number(323, 85, 65, 1);
        flat_label(406, 85, 66, 25, "Top key", 13, SUBTLE);
        let mut pitch = number(472, 85, 56, 83);
        for field in [&mut beat, &mut pitch] {
            field.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
            field.emit(tx, Action::View);
        }
        button(536, 83, 50, "Go", tx, Action::View);
        flat_label(605, 85, 42, 25, "Snap", 13, SUBTLE);
        let mut snap = Choice::new(647, 84, 93, 28, None);
        snap.add_choice("Quarter|Eighth|16th|32nd");
        snap.set_value(2);
        snap.emit(tx, Action::View);
        flat_label(756, 85, 77, 25, "New length", 13, SUBTLE);
        let mut length = Choice::new(834, 84, 93, 28, None);
        length.add_choice("Quarter|Eighth|16th|32nd");
        length.set_value(2);

        let mut grid = Frame::new(249, 120, WIDTH - 265, 502, None);
        grid.set_frame(FrameType::FlatBox);
        let draw_model = Rc::clone(model);
        grid.draw(move |frame| {
            if let Ok(model) = draw_model.try_borrow() {
                draw_grid(frame, &model);
            }
        });
        let handle_model = Rc::clone(model);
        let mut drag = None;
        grid.handle(move |frame, event| {
            let point = || {
                let model = handle_model.try_borrow().ok()?;
                grid_point(frame, &model, app::event_x(), app::event_y())
            };
            match event {
                Event::Push => {
                    let Some(point) = point() else { return false };
                    let _ = frame.take_focus();
                    if app::event_mouse_button() == MouseButton::Right {
                        tx.send(Action::Grid(point, true));
                    } else if app::event_mouse_button() == MouseButton::Left {
                        drag = Some(point);
                    }
                    true
                }
                Event::Drag => drag.is_some(),
                Event::Released => {
                    if let Some(start) = drag.take() {
                        if let Some(end) = point() {
                            if start.key == end.key
                                && start.tick.abs_diff(end.tick)
                                    < handle_model.try_borrow().map_or(1, |model| model.step)
                            {
                                tx.send(Action::Grid(start, false));
                            } else {
                                tx.send(Action::MoveNote(start, end));
                            }
                        }
                    }
                    true
                }
                Event::Focus | Event::Unfocus => true,
                Event::KeyDown if app::event_key() == Key::Delete => {
                    tx.send(Action::DeleteNote);
                    true
                }
                _ => false,
            }
        });
        flat_label(249, 634, 76, 24, "Start tick", 13, SUBTLE);
        let start = number(323, 633, 75, 0);
        flat_label(412, 634, 28, 24, "Key", 13, SUBTLE);
        let key = number(445, 633, 49, 60);
        flat_label(510, 634, 84, 24, "Length ticks", 13, SUBTLE);
        let duration = number(593, 633, 70, 120);
        flat_label(680, 634, 55, 24, "Velocity", 13, SUBTLE);
        let velocity = number(741, 633, 51, 96);
        button(808, 631, 62, "Apply", tx, Action::ApplyNote);
        button(879, 631, 76, "Quantize", tx, Action::SnapNote);
        button(964, 631, 66, "Delete", tx, Action::DeleteNote);
        let mut midi_input = Choice::new(1045, 632, 119, 28, None);
        midi_input.add_choice("MIDI input…");
        midi_input.set_value(0);
        midi_input.set_tooltip("Choose a MIDI input, then press Record.");
        let mut record = button(1039, 83, 125, "Record MIDI", tx, Action::Record);
        record.set_tooltip("Start a new MIDI take. Press Stop to keep it.");
        flat_label(808, 681, 99, 27, "Audio device", 13, SUBTLE);
        let mut audio_device = Choice::new(908, 681, 256, 29, None);
        audio_device.add_choice("System default");
        audio_device.set_value(0);
        audio_device.set_tooltip("Audio destination for the software SoundFont synthesizer.");
        audio_device.emit(tx, Action::OutputChanged);
        let status = flat_label(
            16,
            682,
            780,
            32,
            "Click to add or select a note • Drag to move • Right-click to delete",
            13,
            SUBTLE,
        );

        win.end();
        win.resizable(&grid);
        win.set_callback(move |_| tx.send(Action::Quit));
        Self {
            win,
            tracks,
            name,
            channel,
            patch,
            bank,
            preset,
            presets: Vec::new(),
            volume,
            pan,
            mute,
            solo,
            bpm,
            output,
            audio_device,
            midi_input,
            font,
            looping,
            beat,
            pitch,
            snap,
            length,
            start,
            key,
            duration,
            velocity,
            grid,
            status,
            clock,
        }
    }

    fn sync(&mut self, model: &mut Model) {
        let path = model
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy())
            .unwrap_or_else(|| "Untitled MIDI".into());
        self.win.set_label(&format!(
            "{}{} — Nu Digi Orch",
            if model.dirty { "* " } else { "" },
            path
        ));
        self.tracks.clear();
        for (index, track) in model.song.tracks.iter().enumerate() {
            let suffix = match (track.mute, track.solo) {
                (true, _) => "  [M]",
                (_, true) => "  [S]",
                _ => "",
            };
            self.tracks
                .add(&format!("{:02}  {}{}", index + 1, track.name(), suffix));
        }
        self.tracks.select((model.track + 1) as i32);
        let track = &model.song.tracks[model.track];
        self.name.set_value(&track.name());
        self.channel.set_value(&(track.channel + 1).to_string());
        self.patch.set_value(&track.setting(None).to_string());
        self.bank.set_value(&track.setting(Some(0)).to_string());
        let bank = i32::from(track.setting(Some(0))) + if track.channel == 9 { 128 } else { 0 };
        let program = i32::from(track.setting(None));
        self.preset.set_value(
            self.presets
                .iter()
                .position(|&(b, p)| b == bank && p == program)
                .map_or(0, |i| i as i32 + 1),
        );
        self.volume.set_value(&track.setting(Some(7)).to_string());
        self.pan.set_value(&track.setting(Some(10)).to_string());
        self.mute.set_value(track.mute);
        self.solo.set_value(track.solo);
        self.bpm.set_value(&format!("{:.3}", model.song.bpm()));
        self.beat.set_value(&(model.view_beat + 1).to_string());
        self.pitch.set_value(&model.high_key.to_string());
        model.step = division(model.song.ppq, self.snap.value());
        model.notes = track.notes();
        if let Some(note) = model.selected {
            self.start.set_value(&note.start.to_string());
            self.key.set_value(&note.key.to_string());
            self.duration
                .set_value(&(note.end - note.start).to_string());
            self.velocity.set_value(&note.velocity.to_string());
        }
        self.grid.redraw();
    }

    fn set_font(&mut self, model: &mut Model, path: &Path) -> song::Result<()> {
        let font = audio::load_font(path)?;
        self.presets.clear();
        self.preset.clear();
        self.preset.add_choice("SoundFont presets…");
        for preset in font.get_presets() {
            let bank = preset.get_bank_number();
            let program = preset.get_patch_number();
            if !(0..=255).contains(&bank) || !(0..=127).contains(&program) {
                continue;
            }
            self.presets.push((bank, program));
            let name = menu_label(preset.get_name());
            self.preset.add_choice(&format!("{bank}:{program}  {name}"));
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        self.font
            .set_label(&name.chars().take(23).collect::<String>());
        self.font.set_tooltip(&name);
        model.playback = None;
        model.font = Some(font);
        self.output.set_value(0);
        Ok(())
    }

    fn refresh_outputs(&mut self, model: &mut Model) {
        let prior = self
            .output
            .value()
            .checked_sub(1)
            .and_then(|i| model.outputs.get(i as usize))
            .map(|p| p.name.clone());
        match audio::outputs() {
            Ok(outputs) => model.outputs = outputs,
            Err(error) => {
                model.outputs.clear();
                self.status.set_label(&format!(
                    "MIDI output scan: {error}. SF2 playback is still available."
                ));
            }
        }
        self.output.clear();
        self.output.add_choice("Software SoundFont");
        for device in &model.outputs {
            // Menu paths reserve slash and pipe. Keep each OS device as a single item.
            let name = menu_label(&device.name);
            let label = if device.sound_blaster {
                format!("Sound Blaster · {name}")
            } else {
                name
            };
            self.output.add_choice(&label);
        }
        let index = prior.and_then(|name| model.outputs.iter().position(|port| port.name == name));
        self.output.set_value(index.map_or(0, |i| i as i32 + 1));
        let selected = self.audio_device.value();
        let prior = if selected > 0 {
            model
                .sound_devices
                .get((selected - 1) as usize)
                .map(|port| port.name.clone())
        } else {
            None
        };
        model.sound_devices = audio::sound_devices().unwrap_or_default();
        self.audio_device.clear();
        self.audio_device.add_choice("System default");
        for device in &model.sound_devices {
            let name = menu_label(&device.name);
            let name = if audio::is_sound_blaster(&name) {
                format!("Sound Blaster · {name}")
            } else {
                name
            };
            self.audio_device.add_choice(&name);
        }
        let index = prior.and_then(|name| {
            model
                .sound_devices
                .iter()
                .position(|device| device.name == name)
        });
        self.audio_device
            .set_value(index.map_or(0, |i| i as i32 + 1));
        let prior = model
            .inputs
            .get(self.midi_input.value().saturating_sub(1) as usize)
            .filter(|_| self.midi_input.value() > 0)
            .map(|port| port.name.clone());
        model.inputs = recording::inputs().unwrap_or_default();
        self.midi_input.clear();
        self.midi_input.add_choice("MIDI input…");
        for device in &model.inputs {
            self.midi_input.add_choice(&menu_label(&device.name));
        }
        let index = prior.and_then(|name| model.inputs.iter().position(|port| port.name == name));
        self.midi_input.set_value(index.map_or(0, |i| i as i32 + 1));
    }
}

fn menu_label(label: &str) -> String {
    label
        .replace('/', "⁄")
        .replace('\\', "＼")
        .replace('|', "¦")
        .replace('&', "＆")
}

fn division(ppq: u16, choice: i32) -> Tick {
    (u64::from(ppq) / (1 << choice.clamp(0, 3))).max(1)
}

fn grid_point(grid: &Frame, model: &Model, x: i32, y: i32) -> Option<GridPoint> {
    let x = x - grid.x() - LABEL;
    let y = y - grid.y() - RULER;
    let width = grid.w() - LABEL;
    let height = grid.h() - RULER;
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return None;
    }
    let ppq = u64::from(model.song.ppq);
    Some(GridPoint {
        tick: model.view_beat * ppq + x as u64 * BEATS * ppq / width as u64,
        key: model.high_key - (((y + 1) * ROWS - 1) / height) as u8,
    })
}

fn draw_grid(grid: &Frame, model: &Model) {
    let (x, y, w, h) = (grid.x(), grid.y(), grid.w(), grid.h());
    let width = w - LABEL;
    let height = h - RULER;
    draw::push_clip(x, y, w, h);
    draw::draw_rect_fill(x, y, w, h, PANEL);
    draw::set_font(Font::Helvetica, 12);
    let first_tick = model.view_beat * u64::from(model.song.ppq);
    let shown_ticks = BEATS * u64::from(model.song.ppq);
    for row in 0..ROWS {
        let key = model.high_key - row as u8;
        let black = matches!(key % 12, 1 | 3 | 6 | 8 | 10);
        let top = y + RULER + row * height / ROWS;
        let bottom = y + RULER + (row + 1) * height / ROWS;
        draw::draw_rect_fill(
            x,
            top,
            LABEL,
            bottom - top,
            if black {
                BG
            } else {
                Color::from_rgb(48, 59, 78)
            },
        );
        draw::draw_rect_fill(
            x + LABEL,
            top,
            width,
            bottom - top,
            if black {
                Color::from_rgb(24, 30, 43)
            } else {
                Color::from_rgb(33, 42, 57)
            },
        );
        draw::set_draw_color(Color::from_rgb(53, 66, 85));
        draw::draw_line(x, bottom - 1, x + w, bottom - 1);
        draw::set_draw_color(if key.is_multiple_of(12) {
            ACCENT
        } else {
            SUBTLE
        });
        let name = [
            "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
        ][(key % 12) as usize];
        draw::draw_text2(
            &format!("{name}{}", i32::from(key / 12) - 1),
            x + 5,
            top,
            LABEL - 5,
            bottom - top,
            Align::Inside | Align::Left,
        );
    }
    let next_step = model.step - first_tick % model.step;
    if model.step < u64::from(model.song.ppq) {
        draw::set_draw_color(Color::from_rgb(41, 52, 67));
        for offset in (next_step..shown_ticks).step_by(model.step as usize) {
            let line = x + LABEL + (offset * width as u64 / shown_ticks) as i32;
            draw::draw_line(line, y + RULER, line, y + h);
        }
    }
    for beat in 0..BEATS {
        let line = x + LABEL + (beat * width as u64 / BEATS) as i32;
        let beat = model.view_beat + beat;
        draw::set_draw_color(if beat.is_multiple_of(4) {
            SUBTLE
        } else {
            Color::from_rgb(64, 78, 98)
        });
        draw::draw_line(line, y + RULER, line, y + h);
        draw::set_draw_color(TEXT);
        draw::draw_text2(
            &(beat + 1).to_string(),
            line + 4,
            y,
            48,
            RULER,
            Align::Inside | Align::Left,
        );
    }
    for note in &model.notes {
        if note.key > model.high_key || model.high_key - note.key >= ROWS as u8 {
            continue;
        }
        let end = note.end.max(note.start + 1);
        if end <= first_tick || note.start >= first_tick + shown_ticks {
            continue;
        }
        let left = x
            + LABEL
            + ((note.start.max(first_tick) - first_tick) * width as u64 / shown_ticks) as i32;
        let right = x
            + LABEL
            + ((end.min(first_tick + shown_ticks) - first_tick) * width as u64 / shown_ticks)
                as i32;
        let row = i32::from(model.high_key - note.key);
        let top = y + RULER + row * height / ROWS;
        let bottom = y + RULER + (row + 1) * height / ROWS;
        let color = if model.selected.as_ref() == Some(note) {
            Color::from_rgb(255, 204, 111)
        } else {
            ACCENT
        };
        draw::draw_rect_fill(
            left + 1,
            top + 2,
            (right - left - 2).max(1),
            (bottom - top - 4).max(1),
            color,
        );
    }
    draw::pop_clip();
}

fn number_value(field: &IntInput, min: Tick, max: Tick) -> Option<Tick> {
    field
        .value()
        .trim()
        .parse()
        .ok()
        .filter(|value| (min..=max).contains(value))
}

fn choose(kind: dialog::NativeFileChooserType, title: &str, filter: &str) -> Option<PathBuf> {
    let mut chooser = dialog::NativeFileChooser::new(kind);
    chooser.set_title(title);
    chooser.set_filter(filter);
    if kind == dialog::NativeFileChooserType::BrowseSaveFile {
        chooser.set_option(
            dialog::NativeFileChooserOptions::SaveAsConfirm
                | dialog::NativeFileChooserOptions::UseFilterExt,
        );
    }
    chooser.show();
    let path = chooser.filename();
    (!path.as_os_str().is_empty()).then_some(path)
}

fn browse_save(title: &str, filter: &str, ext: &str) -> Option<PathBuf> {
    let mut path = choose(dialog::NativeFileChooserType::BrowseSaveFile, title, filter)?;
    if path.extension().is_none() {
        path.set_extension(ext);
        if path.exists()
            && dialog::choice2_default("Replace the existing file?", "Cancel", "Replace", "")
                != Some(1)
        {
            return None;
        }
    }
    Some(path)
}

fn discard_ok(model: &Model) -> bool {
    !model.dirty
        || dialog::choice2_default("Discard unsaved MIDI edits?", "Cancel", "Discard", "")
            == Some(1)
}

fn apply(
    action: Action,
    ui: &mut Ui,
    model: &mut Model,
    background: &std::sync::mpsc::Sender<String>,
) -> song::Result<bool> {
    if model.recording.is_some()
        && !matches!(
            action,
            Action::Stop | Action::OutputChanged | Action::View | Action::SelectTrack
        )
    {
        return Err("Press Stop to finish the MIDI take first.".into());
    }
    match action {
        Action::Quit => {
            if model.export_in_progress {
                return Err("Wait for the WAV export to finish before closing.".into());
            }
            if discard_ok(model) {
                model.playback = None;
                ui.win.hide();
                return Ok(false);
            }
        }
        Action::New | Action::Demo | Action::Open => {
            if discard_ok(model) {
                match action {
                    Action::New => model.replace(Song::new(), None, false),
                    Action::Demo => model.replace(Song::demo(), None, true),
                    _ => {
                        if let Some(path) = choose(
                            dialog::NativeFileChooserType::BrowseFile,
                            "Open MIDI",
                            "MIDI\t*.{mid,midi}",
                        ) {
                            model.replace(Song::open(&path)?, Some(path), false);
                        }
                    }
                }
            }
        }
        Action::Save | Action::SaveAs => {
            let path = if matches!(action, Action::Save) && model.path.is_some() {
                model.path.clone()
            } else {
                browse_save("Save MIDI", "MIDI\t*.mid", "mid")
            };
            if let Some(path) = path {
                model.song.save(&path)?;
                model.path = Some(path);
                model.dirty = false;
                ui.status.set_label("MIDI saved.");
            }
        }
        Action::Export => {
            if model.export_in_progress {
                return Err("A WAV export is already running.".into());
            }
            let font = Arc::clone(
                model
                    .font
                    .as_ref()
                    .ok_or("Load an SF2 SoundFont before exporting WAV.")?,
            );
            if let Some(path) = browse_save("Export WAV", "Wave audio\t*.wav", "wav") {
                let timeline = model.song.timeline();
                let background = background.clone();
                std::thread::Builder::new()
                    .name("wav-export".into())
                    .spawn(move || {
                        let status = match audio::export_wav(&path, &font, timeline) {
                            Ok(()) => format!("Exported {}", path.display()),
                            Err(error) => format!("Could not export WAV: {error}"),
                        };
                        let _ = background.send(status);
                        app::awake();
                    })?;
                model.export_in_progress = true;
                ui.status.set_label("Rendering WAV…");
            }
        }
        Action::Undo => model.history(false),
        Action::Redo => model.history(true),
        Action::AddTrack => {
            model.checkpoint();
            model.song.add_track();
            model.track = model.song.tracks.len() - 1;
            model.selected = None;
        }
        Action::RemoveTrack => {
            model.checkpoint();
            model.song.remove_track(model.track);
            model.track = model.track.min(model.song.tracks.len() - 1);
            model.selected = None;
        }
        Action::SelectTrack => {
            if let Some(index) = ui
                .tracks
                .value()
                .checked_sub(1)
                .and_then(|index| usize::try_from(index).ok())
            {
                if index < model.song.tracks.len() && model.track != index {
                    model.track = index;
                    model.selected = None;
                    model.fit_pitch();
                }
            }
        }
        Action::TrackSettings => {
            let channel = number_value(&ui.channel, 1, 16)
                .ok_or("MIDI channel must be between 1 and 16.")? as u8
                - 1;
            let patch =
                number_value(&ui.patch, 0, 127).ok_or("Program must be between 0 and 127.")? as u8;
            let bank =
                number_value(&ui.bank, 0, 127).ok_or("Bank must be between 0 and 127.")? as u8;
            let volume =
                number_value(&ui.volume, 0, 127).ok_or("Volume must be between 0 and 127.")? as u8;
            let pan = number_value(&ui.pan, 0, 127).ok_or("Pan must be between 0 and 127.")? as u8;
            model.checkpoint();
            let track = &mut model.song.tracks[model.track];
            let name = ui.name.value();
            if name.trim() != track.name() && !name.trim().is_empty() {
                track.rename(name.trim());
            }
            track.set_channel(channel);
            for (cc, value) in [
                (Some(0), bank),
                (None, patch),
                (Some(7), volume),
                (Some(10), pan),
            ] {
                if track.setting(cc) != value {
                    track.set_setting(cc, value);
                }
            }
            track.mute = ui.mute.value();
            track.solo = ui.solo.value();
            model.selected = None;
        }
        Action::Preset => {
            if let Some(&(bank, program)) = ui
                .presets
                .get(ui.preset.value().saturating_sub(1) as usize)
                .filter(|_| ui.preset.value() > 0)
            {
                let channel = model.song.tracks[model.track].channel;
                if (bank >= 128) != (channel == 9) {
                    ui.sync(model);
                    return Err("For drum presets set track channel 10. Use another channel for melodic presets.".into());
                }
                model.checkpoint();
                let track = &mut model.song.tracks[model.track];
                track.set_setting(Some(0), (bank % 128) as u8);
                track.set_setting(None, program as u8);
                model.selected = None;
            }
        }
        Action::Tempo => {
            let bpm: f64 = ui
                .bpm
                .value()
                .trim()
                .parse()
                .map_err(|_| "Tempo must be between 4 and 999 BPM.")?;
            if !(4.0..=999.0).contains(&bpm) {
                return Err("Tempo must be between 4 and 999 BPM.".into());
            }
            model.checkpoint();
            model.song.set_tempo(bpm);
            model.selected = None;
        }
        Action::View => {
            model.view_beat = number_value(&ui.beat, 1, 25_000_000)
                .ok_or("First beat must be between 1 and 25,000,000.")?
                - 1;
            model.high_key = number_value(&ui.pitch, 23, 127)
                .ok_or("Top key must be between 23 and 127.")? as u8;
        }
        Action::LoadFont => {
            if let Some(path) = choose(
                dialog::NativeFileChooserType::BrowseFile,
                "Load SoundFont",
                "SoundFont 2\t*.sf2",
            ) {
                ui.set_font(model, &path)?;
                ui.status.set_label("SoundFont loaded.");
            }
        }
        Action::Refresh => {
            model.playback = None;
            ui.refresh_outputs(model);
        }
        Action::Play => {
            let timeline = model.song.timeline();
            if timeline.packets.is_empty() || timeline.duration == 0 {
                return Err("Add notes to an audible track before playing.".into());
            }
            model.playback = None;
            let looping = ui.looping.value();
            model.playback = Some(if ui.output.value() > 0 {
                let port = model
                    .outputs
                    .get(ui.output.value() as usize - 1)
                    .ok_or("Refresh MIDI outputs and choose a device.")?;
                audio::Playback::hardware(port.port.clone(), timeline, looping)?
            } else {
                let font = model
                    .font
                    .as_ref()
                    .ok_or("Choose Load SF2 to load a SoundFont, or select a MIDI output.")?;
                let index = ui.audio_device.value();
                let device = if index > 0 {
                    model.sound_devices.get((index - 1) as usize)
                } else {
                    None
                };
                audio::Playback::software(font, timeline, looping, device)?
            });
            ui.status.set_label("Playing");
        }
        Action::Record => {
            let port = ui.midi_input.value().checked_sub(1).and_then(|index| model.inputs.get(index as usize))
                .ok_or("Choose a MIDI input at the bottom right. Use Playback → Refresh Devices if needed.")?;
            model.playback = None;
            model.captured.clear();
            model.recording = Some(recording::Recording::start(&port.port)?);
            ui.status
                .set_label("Recording MIDI from the start. Press Stop to keep the take.");
        }
        Action::OutputChanged => {
            if model.playback.take().is_some() {
                ui.clock.set_label("00:00.0");
                ui.status
                    .set_label("Playback stopped after changing the output.");
            }
        }
        Action::Stop => {
            if let Some(recording) = model.recording.take() {
                let (duration, events) = recording.finish(std::mem::take(&mut model.captured));
                if events.is_empty() {
                    ui.status.set_label("The MIDI take was empty.");
                } else {
                    model.checkpoint();
                    model.track = model.song.add_recording(duration, events);
                    model.fit_pitch();
                    model.selected = None;
                    model.view_beat = 0;
                    ui.status.set_label("MIDI take added.");
                }
                ui.clock.set_label("00:00.0");
                return Ok(true);
            }
            model.playback = None;
            ui.clock.set_label("00:00.0");
            ui.status.set_label("Stopped");
        }
        Action::Grid(point, delete) => {
            if let Some(note) = model.hit(point) {
                if delete {
                    model.checkpoint();
                    model.song.tracks[model.track].delete_note(note);
                    model.selected = None;
                } else {
                    model.selected = Some(note);
                }
            } else if !delete {
                let start = point.tick / model.step * model.step;
                let end = start + division(model.song.ppq, ui.length.value());
                if let Some(note) = model.notes.iter().find(|note| {
                    note.key == point.key
                        && note.channel == model.song.tracks[model.track].channel
                        && note.start < end
                        && note.end > start
                }) {
                    model.selected = Some(*note);
                } else {
                    let velocity = number_value(&ui.velocity, 1, 127).unwrap_or(96) as u8;
                    model.checkpoint();
                    model.selected = Some(
                        model.song.tracks[model.track].add_note(start, end, point.key, velocity),
                    );
                }
            }
        }
        Action::MoveNote(from, to) => {
            if let Some(note) = model.hit(from) {
                let start = note
                    .start
                    .saturating_add_signed(to.tick as i64 - from.tick as i64);
                let start = (start + model.step / 2) / model.step * model.step;
                let key = (i16::from(note.key) + i16::from(to.key) - i16::from(from.key))
                    .clamp(0, 127) as u8;
                update_note(
                    model,
                    note,
                    Note {
                        start,
                        end: start + (note.end - note.start).max(1),
                        key,
                        ..note
                    },
                )?;
            }
        }
        Action::ApplyNote | Action::SnapNote => {
            let note = model.selected.ok_or("Select a note first.")?;
            let changed = if matches!(action, Action::SnapNote) {
                let start = (note.start + model.step / 2) / model.step * model.step;
                Note {
                    start,
                    end: start + (note.end - note.start).max(1),
                    ..note
                }
            } else {
                let start = number_value(&ui.start, 0, i64::MAX as u64)
                    .ok_or("Enter a valid start tick.")?;
                let duration = number_value(&ui.duration, 1, 0x0fff_ffff)
                    .ok_or("Enter a positive MIDI note length.")?;
                let key =
                    number_value(&ui.key, 0, 127).ok_or("Key must be between 0 and 127.")? as u8;
                let velocity = number_value(&ui.velocity, 1, 127)
                    .ok_or("Velocity must be between 1 and 127.")?
                    as u8;
                Note {
                    start,
                    end: start + duration,
                    key,
                    velocity,
                    ..note
                }
            };
            update_note(model, note, changed)?;
        }
        Action::DeleteNote => {
            if let Some(note) = model.selected.take() {
                model.checkpoint();
                model.song.tracks[model.track].delete_note(note);
            }
        }
    }
    Ok(true)
}

fn update_note(model: &mut Model, original: Note, changed: Note) -> song::Result<()> {
    if original == changed {
        return Ok(());
    }
    if model.notes.iter().any(|note| {
        note.on != original.on
            && note.channel == changed.channel
            && note.key == changed.key
            && note.start < changed.end
            && note.end > changed.start
    }) {
        return Err("This edit would overlap a note of the same pitch and MIDI channel.".into());
    }
    model.checkpoint();
    model.song.tracks[model.track].change_note(changed);
    model.selected = Some(changed);
    Ok(())
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments
        .first()
        .is_some_and(|arg| arg.to_string_lossy().starts_with("--"))
    {
        if let Err(error) = run_command(&arguments) {
            eprintln!("Nu Digi Orch: {error}");
            std::process::exit(1);
        }
        return;
    }
    let _app = app::App::default();
    let (send, receive) = app::channel::<Action>();
    let (background, messages) = std::sync::mpsc::channel();
    let model = Rc::new(RefCell::new(Model::new()));
    let mut ui = Ui::new(send, &model);
    {
        let mut model = model.borrow_mut();
        if let Some(path) = arguments.first().map(PathBuf::from) {
            match Song::open(&path) {
                Ok(song) => model.replace(song, Some(path), false),
                Err(error) => ui.status.set_label(&format!("Couldn't open MIDI: {error}")),
            }
        }
        ui.refresh_outputs(&mut model);
        if let Some(path) = default_soundfont() {
            if let Err(error) = ui.set_font(&mut model, &path) {
                ui.status
                    .set_label(&format!("Could not load default SoundFont: {error}"));
            }
        }
        ui.sync(&mut model);
    }
    ui.win.show();
    while ui.win.shown() {
        let _ = app::wait_for(0.05);
        while let Some(action) = receive.recv() {
            // Native dialogs can dispatch window events. Grid drawing uses try_borrow while an action runs.
            let mut model = model.borrow_mut();
            match apply(action, &mut ui, &mut model, &background) {
                Ok(false) => return,
                Ok(true) => ui.sync(&mut model),
                Err(error) => ui.status.set_label(&error.to_string()),
            }
        }
        let mut model = model.borrow_mut();
        while let Ok(message) = messages.try_recv() {
            model.export_in_progress = false;
            ui.status.set_label(&message);
        }
        if model.recording.is_some() {
            let model = &mut *model;
            if let Some(recording) = &model.recording {
                recording.drain(&mut model.captured);
                let elapsed = recording.elapsed() / 100_000;
                ui.clock.set_label(&format!(
                    "{:02}:{:02}.{}",
                    elapsed / 600,
                    elapsed / 10 % 60,
                    elapsed % 10
                ));
            }
        } else if let Some(playback) = &model.playback {
            let elapsed = playback.progress.position.load(Ordering::Relaxed) / 100_000;
            let label = format!(
                "{:02}:{:02}.{}",
                elapsed / 600,
                elapsed / 10 % 60,
                elapsed % 10
            );
            if ui.clock.label() != label {
                ui.clock.set_label(&label);
            }
            if let Ok(error) = playback.errors.try_recv() {
                model.playback = None;
                ui.status.set_label(&error);
            } else if playback.progress.done.load(Ordering::Relaxed) {
                model.playback = None;
                ui.status.set_label("Playback finished.");
            }
        }
    }
}

fn default_soundfont() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NU_DIGI_ORCH_SF2") {
        return Some(path.into());
    }
    [
        "/usr/share/sounds/sf2/default-GM.sf2",
        "/usr/share/sounds/sf2/FluidR3_GM.sf2",
        "/usr/share/soundfonts/default.sf2",
        "/usr/share/soundfonts/FluidR3_GM.sf2",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

fn run_command(arguments: &[OsString]) -> song::Result<()> {
    match arguments {
        [command] if command == "--help" => {
            println!("nu-digi-orch [INPUT.mid]\nnu-digi-orch --demo-midi OUTPUT.mid\nnu-digi-orch --render INPUT.mid FONT.sf2 OUTPUT.wav");
            Ok(())
        }
        [command, output] if command == "--demo-midi" => Song::demo().save(Path::new(output)),
        [command, input, font, output] if command == "--render" => {
            let song = Song::open(Path::new(input))?;
            let font = audio::load_font(Path::new(font))?;
            audio::export_wav(Path::new(output), &font, song.timeline())
        }
        _ => Err("Invalid command. Run with --help for usage.".into()),
    }
}
