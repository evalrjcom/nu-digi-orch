#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod bus;
mod effects;
mod gm;
mod harmony;
mod keyboard;
mod mixer;
mod recording;
mod rhythm;
mod song;
mod track_view;

use fltk::{
    app::{self, MouseButton},
    button::Button,
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
const CHORD_BAND: i32 = 21;
const RULER: i32 = 26 + CHORD_BAND;
const SCROLL: i32 = 16;
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
    Cell(usize, usize),
    Preset,
    BrowsePresets,
    TogglePlayback,
    VirtualKeyboard,
    TrackEffect(usize, usize, u8),
    Jump(i8),
    Tempo,
    View,
    ScrollTime(u64),
    ScrollTracks(usize),
    ScrollPitch(u8),
    ScrollMixer(usize),
    ResizeScrollbars,
    ToggleEditor,
    BusValue(usize, usize, i16),
    MixPreset(bool, i8, i8, u8),
    Effects,
    MixerValue(usize, usize, u8),
    Seek(Tick),
    Step(usize, usize, i16),
    FocusCell(usize, usize),
    TempoStep(i8),
    Lane(usize, bool),
    Play,
    Record,
    RecordSelected,
    RecordBacking,
    MetronomeChanged,
    CountIn(u8),
    CountSound(rhythm::Pattern),
    KeySignature,
    Transpose,
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
    effects: Arc<effects::Levels>,
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
    record_target: Option<usize>,
    record_offset: u64,
    record_lead: u64,
    count_beats: u8,
    count_sound: rhythm::Pattern,
    click: Option<rhythm::Pattern>,
    export_in_progress: bool,
    inputs: Vec<recording::InputPort>,
    view_beat: Tick,
    track_scroll: usize,
    last_beat: Tick,
    follow_playhead: bool,
    mixer_scroll: usize,
    chords: Vec<(Tick, Option<harmony::Chord>)>,
    keys: Vec<(Tick, (i8, bool))>,
    track_view: bool,
    mixer: bool,
    tracks_only: bool,
    column: usize,
    cue: Tick,
    jump_cue: Option<u64>,
    offset: u64,
    lanes: Vec<Vec<Note>>,
    playhead: Tick,
    tick_at_time: Box<dyn Fn(u64) -> Tick>,
    duration_micros: u64,
    high_key: u8,
    step: Tick,
}

impl Model {
    fn new() -> Self {
        let song = Song::new();
        Self {
            step: u64::from(song.ppq) / 4,
            effects: Arc::default(),
            tick_at_time: Box::new(song.tempo_clock()),
            track_view: true,
            mixer: false,
            tracks_only: false,
            column: 2,
            cue: 0,
            jump_cue: None,
            offset: 0,
            lanes: vec![],
            playhead: 0,
            duration_micros: 0,
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
            record_target: None,
            record_offset: 0,
            record_lead: 0,
            count_beats: 0,
            count_sound: rhythm::Pattern::Click,
            click: None,
            export_in_progress: false,
            inputs: Vec::new(),
            view_beat: 0,
            track_scroll: 0,
            last_beat: 0,
            follow_playhead: true,
            mixer_scroll: 0,
            chords: Vec::new(),
            keys: Vec::new(),
            high_key: 83,
        }
    }
    fn checkpoint(&mut self) {
        self.playback = None;
        self.save_undo();
    }
    fn save_undo(&mut self) {
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
        self.cue = 0;
        self.jump_cue = None;
        self.playhead = 0;
        self.song = song;
        self.path = path;
        self.dirty = dirty;
        self.track = 0;
        self.selected = None;
        self.undo.clear();
        self.redo.clear();
        self.view_beat = 0;
        self.follow_playhead = true;
        self.track_scroll = 0;
        self.mixer_scroll = 0;
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
    effects: Window,
    win: Window,
    arrangement: Frame,
    preset: fltk::browser::HoldBrowser,
    preset_window: Window,
    keyboard: keyboard::Keyboard,
    last_stop: Option<std::time::Instant>,
    presets: Vec<(i32, i32)>,
    bpm: Input,
    output: Choice,
    audio_device: Choice,
    midi_input: Choice,
    font: Frame,
    looping: fltk::button::ToggleButton,
    metronome: fltk::button::ToggleButton,
    rhythm: Choice,
    beat: IntInput,
    pitch: IntInput,
    snap: Choice,
    length: Choice,
    start: IntInput,
    key: IntInput,
    duration: IntInput,
    velocity: IntInput,
    grid: Frame,
    toggle_editor: Choice,
    piano_tools: Vec<fltk::widget::Widget>,
    navigation_tools: Vec<fltk::widget::Widget>,

    status: Frame,
    clock: Frame,
    chord: Frame,
    piano_dock: Button,
    time_scroll: fltk::valuator::Scrollbar,
    track_scroll: fltk::valuator::Scrollbar,
    pitch_scroll: fltk::valuator::Scrollbar,
    mixer_scroll: fltk::valuator::Scrollbar,
}

impl Ui {
    fn new(tx: app::Sender<Action>, model: &Rc<RefCell<Model>>) -> Self {
        app::set_scheme(app::Scheme::Gleam);
        app::background(36, 44, 60);
        app::background2(25, 32, 47);
        app::foreground(229, 236, 246);
        let mut win = Window::new(80, 55, WIDTH, HEIGHT, "Nu Digi Orch");
        win.set_color(BG);
        win.size_range(1160, 690, 0, 0);

        let mut menu = MenuBar::new(0, 0, 785, 36, None);
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
            ("Edit/Set key signature…", None, Action::KeySignature),
            ("Edit/Transpose song…", None, Action::Transpose),
            ("Playback/Count-in/Off", None, Action::CountIn(0)),
            ("Playback/Count-in/1 bar", None, Action::CountIn(4)),
            ("Playback/Count-in/2 bars", None, Action::CountIn(8)),
            (
                "Playback/Count-in sound/Click",
                None,
                Action::CountSound(rhythm::Pattern::Click),
            ),
            (
                "Playback/Count-in sound/Cowbell",
                None,
                Action::CountSound(rhythm::Pattern::Cowbell),
            ),
            (
                "Playback/Count-in sound/Side stick",
                None,
                Action::CountSound(rhythm::Pattern::SideStick),
            ),
            ("Edit/Delete Note", None, Action::DeleteNote),
            ("Edit/Quantize Selected Note", None, Action::SnapNote),
            (
                "Track/Record into selected track",
                Some('r'),
                Action::RecordSelected,
            ),
            ("Track/Add Track", None, Action::AddTrack),
            ("Track/Remove Track", None, Action::RemoveTrack),
            (
                "Mixer/Track presets/Flat",
                None,
                Action::MixPreset(false, 0, 0, 100),
            ),
            (
                "Mixer/Track presets/Vocal presence",
                None,
                Action::MixPreset(false, -3, 3, 100),
            ),
            (
                "Mixer/Track presets/Bass weight",
                None,
                Action::MixPreset(false, 4, -2, 100),
            ),
            (
                "Mixer/Track presets/Acoustic sparkle",
                None,
                Action::MixPreset(false, -2, 4, 100),
            ),
            (
                "Mixer/Master presets/Flat unity",
                None,
                Action::MixPreset(true, 0, 0, 100),
            ),
            (
                "Mixer/Master presets/Warm tape EQ",
                None,
                Action::MixPreset(true, 2, -2, 90),
            ),
            (
                "Mixer/Master presets/Gentle smile EQ",
                None,
                Action::MixPreset(true, 2, 2, 80),
            ),
            (
                "Mixer/Master presets/Bright clarity",
                None,
                Action::MixPreset(true, -1, 2, 85),
            ),
            ("Playback/SoundFont effects…", None, Action::Effects),
            (
                "Playback/Virtual piano…",
                Some('k'),
                Action::VirtualKeyboard,
            ),
            ("Playback/Load SoundFont…", None, Action::LoadFont),
            ("Playback/Refresh Devices", None, Action::Refresh),
        ] {
            menu_item(&mut menu, path, key, tx, action);
        }

        let mut play = button(16, 43, 45, "@>", tx, Action::Play);
        play.set_color(ACCENT);
        play.set_label_color(BG);
        button(65, 43, 45, "@square", tx, Action::Stop).set_tooltip("Stop");
        play.set_tooltip("Play");
        let looping = fltk::button::ToggleButton::new(165, 43, 68, 29, "Loop");
        let clock = flat_label(246, 43, 100, 29, "00:00.0", 17, ACCENT);
        let mut chord = flat_label(344, 43, 125, 29, "—", 13, TEXT);
        chord.set_frame(FrameType::DownBox);
        chord.set_color(PANEL);
        chord.set_tooltip("Chord and MIDI key signature at the playhead");
        flat_label(480, 43, 40, 29, "BPM", 13, SUBTLE);
        let mut bpm = Input::new(521, 43, 70, 30, None);
        bpm.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
        bpm.emit(tx, Action::Tempo);
        for (y, symbol, step, hint) in [
            (43, "@8>", 1, "Increase tempo by 1 BPM"),
            (58, "@2>", -1, "Decrease tempo by 1 BPM"),
        ] {
            let mut arrow = fltk::button::RepeatButton::new(592, y, 25, 15, symbol);
            arrow.set_label_size(9);
            arrow.set_tooltip(hint);
            arrow.emit(tx, Action::TempoStep(step));
        }
        flat_label(625, 43, 53, 29, "Output", 13, SUBTLE);
        let mut output = Choice::new(676, 43, 136, 29, None);
        output.add_choice("Software SoundFont");
        output.set_value(0);
        output.emit(tx, Action::OutputChanged);
        button(821, 43, 92, "Load SF2…", tx, Action::LoadFont);
        let font = flat_label(921, 43, 100, 29, "Choose an SF2 file", 12, SUBTLE);

        flat_label(16, 85, 37, 25, "View", 13, SUBTLE);
        button(232, 83, 68, "+ Track", tx, Action::AddTrack);
        button(306, 83, 32, "−", tx, Action::RemoveTrack).set_tooltip("Remove selected track");
        button(
            16,
            633,
            215,
            "SoundFont presets…",
            tx,
            Action::BrowsePresets,
        );
        menu.add(
            "Playback/Play or pause",
            Shortcut::None,
            MenuFlag::Normal,
            move |_| tx.send(Action::TogglePlayback),
        );

        let first_label = flat_label(362, 85, 66, 25, "First beat", 13, SUBTLE);
        let mut beat = number(428, 85, 62, 1);
        let pitch_label = flat_label(574, 85, 62, 25, "Top key", 13, SUBTLE);
        let mut pitch = number(636, 85, 50, 83);
        for field in [&mut beat, &mut pitch] {
            field.set_trigger(fltk::enums::CallbackTrigger::EnterKey);
            field.emit(tx, Action::View);
        }
        let go = button(496, 83, 48, "Go", tx, Action::View);
        let snap_label = flat_label(710, 85, 42, 25, "Snap", 13, SUBTLE);
        let mut snap = Choice::new(752, 84, 93, 28, None);
        snap.add_choice("Quarter|Eighth|16th|32nd");
        snap.set_value(2);
        snap.emit(tx, Action::View);
        let length_label = flat_label(872, 85, 77, 25, "New length", 13, SUBTLE);
        let mut length = Choice::new(951, 84, 93, 28, None);
        length.add_choice("Quarter|Eighth|16th|32nd");
        length.set_value(2);

        let mut arrangement = Frame::new(16, 120, WIDTH - 32, 230, None);
        let data = Rc::clone(model);
        arrangement.draw(move |f| {
            if let Ok(m) = data.try_borrow() {
                if m.mixer {
                    mixer::paint(f, &m);
                    return;
                }
                track_view::draw_tracks(f, &m);
                track_view::draw_playhead(f, &m, track_view::GUTTER);
            }
        });
        let data = Rc::clone(model);
        let mut mixer_drag = None;
        arrangement.handle(move |f, e| {
            data.try_borrow().is_ok_and(|m| {
                if m.mixer {
                    mixer::handle(f, &m, e, tx, &mut mixer_drag)
                } else {
                    track_view::handle(f, &m, e, tx)
                }
            })
        });
        let mut grid = Frame::new(16, 350, WIDTH - 32, 272, None);
        grid.set_frame(FrameType::FlatBox);
        let data = Rc::clone(model);
        grid.draw(move |f| {
            if let Ok(m) = data.try_borrow() {
                draw_grid(f, &m);
                track_view::draw_playhead(f, &m, roll_gutter(&m));
            }
        });
        let handle_model = Rc::clone(model);
        let mut drag = None;
        grid.handle(move |frame, event| {
            if let Ok(m) = handle_model.try_borrow() {
                if track_view::seek_ruler(frame, &m, event, roll_gutter(&m), tx) {
                    return true;
                }
            }
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
                Event::MouseWheel => {
                    if let Ok(model) = handle_model.try_borrow() {
                        let key = i32::from(model.high_key) - app::event_dy_value();
                        tx.send(Action::ScrollPitch(key.clamp(ROWS - 1, 127) as u8));
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
        let start_label = flat_label(249, 634, 76, 24, "Start tick", 13, SUBTLE);
        let start = number(323, 633, 75, 0);
        let key_label = flat_label(412, 634, 28, 24, "Key", 13, SUBTLE);
        let key = number(445, 633, 49, 60);
        let duration_label = flat_label(510, 634, 84, 24, "Length ticks", 13, SUBTLE);
        let duration = number(593, 633, 70, 120);
        let velocity_label = flat_label(680, 634, 55, 24, "Velocity", 13, SUBTLE);
        let velocity = number(741, 633, 51, 96);
        let apply_note = button(808, 631, 62, "Apply", tx, Action::ApplyNote);
        let quantize = button(879, 631, 76, "Quantize", tx, Action::SnapNote);
        let delete = button(964, 631, 66, "Delete", tx, Action::DeleteNote);
        let mut toggle_editor = Choice::new(55, 83, 158, 29, None);
        toggle_editor.add_choice("Split view|Full track view|Full piano roll|Track mixer");
        toggle_editor.set_value(0);
        toggle_editor.emit(tx, Action::ToggleEditor);
        let mut midi_input = Choice::new(797, 7, 168, 28, None);
        midi_input.add_choice("MIDI input…");
        midi_input.set_value(0);
        midi_input.set_tooltip("Choose a MIDI input, then press Record.");
        let mut metronome = fltk::button::ToggleButton::new(1030, 43, 116, 29, "Metronome");
        metronome.set_tooltip("Toggle the click or drum pattern, including during playback.");
        metronome.emit(tx, Action::MetronomeChanged);
        let mut rhythm = Choice::new(1049, 84, 115, 28, None);
        rhythm.add_choice(rhythm::Pattern::LABELS);
        rhythm.set_value(0);
        rhythm.set_tooltip("Metronome sound or drum pattern");
        rhythm.emit(tx, Action::MetronomeChanged);
        let mut record = button(114, 43, 45, "@circle", tx, Action::Record);
        record.set_label_color(Color::Red);
        record.set_tooltip("Start a new MIDI take. Press Stop to keep it.");

        let mut audio_device = Choice::new(974, 7, 190, 29, None);
        audio_device.add_choice("System default");
        audio_device.set_value(0);
        audio_device.set_tooltip("Audio destination for the software SoundFont synthesizer.");
        audio_device.emit(tx, Action::OutputChanged);
        let piano_dock = button(1035, 631, 129, "Virtual piano", tx, Action::VirtualKeyboard);
        let status = flat_label(
            16,
            682,
            570,
            32,
            "Select a track or double-click a lane to edit its notes.",
            13,
            SUBTLE,
        );

        let piano_tools = vec![
            pitch_label.as_base_widget(),
            pitch.as_base_widget(),
            snap_label.as_base_widget(),
            snap.as_base_widget(),
            length_label.as_base_widget(),
            length.as_base_widget(),
            start_label.as_base_widget(),
            start.as_base_widget(),
            key_label.as_base_widget(),
            key.as_base_widget(),
            duration_label.as_base_widget(),
            duration.as_base_widget(),
            velocity_label.as_base_widget(),
            velocity.as_base_widget(),
            apply_note.as_base_widget(),
            quantize.as_base_widget(),
            delete.as_base_widget(),
        ];
        let navigation_tools = vec![
            first_label.as_base_widget(),
            beat.as_base_widget(),
            go.as_base_widget(),
        ];
        use fltk::valuator::{Scrollbar, ScrollbarType};
        let mut time_scroll = Scrollbar::new(16, 606, WIDTH - 32, SCROLL, None);
        time_scroll.set_type(ScrollbarType::Horizontal);
        time_scroll.set_callback(move |bar| tx.send(Action::ScrollTime(bar.value() as u64)));
        time_scroll.set_linesize(1);
        let mut track_scroll = Scrollbar::new(WIDTH - 32, 120, SCROLL, 220, None);
        track_scroll.set_linesize(1);
        track_scroll.set_callback(move |bar| tx.send(Action::ScrollTracks(bar.value() as usize)));
        let mut pitch_scroll = Scrollbar::new(WIDTH - 32, 340, SCROLL, 266, None);
        pitch_scroll.set_linesize(1);
        pitch_scroll.set_callback(move |bar| tx.send(Action::ScrollPitch(127 - bar.value() as u8)));
        let mut mixer_scroll = Scrollbar::new(16, 606, WIDTH - 32, SCROLL, None);
        mixer_scroll.set_type(ScrollbarType::Horizontal);
        mixer_scroll.set_linesize(1);
        mixer_scroll.set_callback(move |bar| tx.send(Action::ScrollMixer(bar.value() as usize)));
        win.end();
        // Keep controls at native size; only the editor expands across a wide window.
        let anchored: Vec<_> = (0..win.children())
            .filter_map(|i| win.child(i))
            .map(|widget| {
                let bounds = (widget.x(), widget.y(), widget.w(), widget.h());
                (widget, bounds)
            })
            .collect();
        win.resizable(&grid);
        let layout_grid = grid.clone();
        let layout_tracks = arrangement.clone();
        let (mut time_bar, mut track_bar, mut pitch_bar, mut mixer_bar) = (
            time_scroll.clone(),
            track_scroll.clone(),
            pitch_scroll.clone(),
            mixer_scroll.clone(),
        );
        let data = model.clone();
        win.resize_callback(move |_, _, _, w, h| {
            for (widget, (x, y, width, height)) in &anchored {
                let mut widget = widget.clone();
                let right = *y == 7 && *x >= 797;
                let px = if right { x + w - WIDTH } else { *x };
                let py = if *y >= 631 { y + h - HEIGHT } else { *y };
                widget.resize(px, py, if *y == 0 { w - 395 } else { *width }, *height);
            }
            let mut grid = layout_grid.clone();
            let mut tracks = layout_tracks.clone();
            let height = h - 228 - SCROLL;
            let split = if !grid.visible() {
                height
            } else if tracks.visible() {
                height * 45 / 100
            } else {
                0
            };
            let mixer = data.try_borrow().is_ok_and(|model| model.mixer);
            let width = w - 32 - if mixer { 0 } else { SCROLL };
            tracks.resize(16, 120, width, split.max(1));
            grid.resize(16, 120 + split, width, (height - split).max(1));
            time_bar.resize(16, 120 + height, width, SCROLL);
            mixer_bar.resize(16, 120 + height, width, SCROLL);
            track_bar.resize(16 + width, 120 + RULER, SCROLL, (split - RULER).max(1));
            tx.send(Action::ResizeScrollbars);
            pitch_bar.resize(
                16 + width,
                120 + split + RULER,
                SCROLL,
                (height - split - RULER).max(1),
            );
        });
        win.set_callback(move |_| tx.send(Action::Quit));
        let mut effects = Window::new(180, 160, 400, 188, "SoundFont effects");
        effects.set_color(BG);
        for (index, label) in ["Reverb", "Chorus", "Delay"].into_iter().enumerate() {
            let y = 14 + index as i32 * 56;
            flat_label(16, y, 75, 32, label, 14, TEXT);
            let mut slider = fltk::valuator::HorNiceSlider::new(96, y, 234, 32, None);
            slider.set_bounds(0., 100.);
            slider.set_step(1., 1);
            let mut readout = flat_label(342, y, 52, 32, "0%", 14, ACCENT);
            let levels = model.borrow().effects.clone();
            slider.set_callback(move |s| {
                let amount = s.value().round() as u8;
                levels.set(index, amount);
                readout.set_label(&format!("{amount}%"));
            });
        }
        effects.end();
        let preset_window = Window::new(130, 120, 590, 510, "SoundFont presets · Bank / Patch");
        let preset = fltk::browser::HoldBrowser::new(12, 12, 566, 442, None);
        preset_window.resizable(&preset);
        button(12, 466, 150, "Apply to track", tx, Action::Preset);
        button(174, 466, 150, "Load SF2…", tx, Action::LoadFont);
        preset_window.end();
        Self {
            effects,
            win,
            arrangement,
            preset,
            preset_window,
            keyboard: keyboard::Keyboard::new(tx),
            last_stop: None,
            presets: Vec::new(),
            bpm,
            output,
            audio_device,
            midi_input,
            font,
            looping,
            metronome,
            rhythm,
            beat,
            pitch,
            snap,
            length,
            start,
            key,
            duration,
            velocity,
            grid,
            toggle_editor,
            navigation_tools,
            piano_tools,

            status,
            clock,
            chord,
            piano_dock,
            time_scroll,
            track_scroll,
            pitch_scroll,
            mixer_scroll,
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
        let track = &model.song.tracks[model.track];
        let bank = i32::from(track.setting(Some(0))) + if track.channel == 9 { 128 } else { 0 };
        let program = i32::from(track.setting(None));
        self.preset.select(
            self.presets
                .iter()
                .position(|&(b, p)| b == bank && p == program)
                .map_or(0, |i| i as i32 + 1),
        );
        self.bpm.set_value(&format!("{:.3}", model.song.bpm()));
        self.beat.set_value(&(model.view_beat + 1).to_string());
        self.pitch.set_value(&model.high_key.to_string());
        model.step = division(model.song.ppq, self.snap.value());
        if model.mixer {
            model.lanes.clear();
            model.notes.clear();
        } else if model.track_view {
            model.lanes = model
                .song
                .tracks
                .iter()
                .map(|track| track.notes())
                .collect();
            model.notes.clone_from(&model.lanes[model.track]);
        } else {
            model.notes = track.notes();
            model.lanes.clear();
        }
        let width = self.win.w() - 32 - if model.mixer { 0 } else { SCROLL };
        let height = self.win.h() - 228 - SCROLL;
        let split = if model.tracks_only {
            height
        } else if model.track_view {
            height * 45 / 100
        } else {
            0
        };
        self.arrangement.resize(16, 120, width, split.max(1));
        if model.track_view {
            self.arrangement.show();
        } else {
            self.arrangement.hide();
        }
        self.grid
            .resize(16, 120 + split, width, (height - split).max(1));
        if model.tracks_only {
            self.grid.hide();
        } else {
            self.grid.show();
        }
        self.time_scroll.resize(16, 120 + height, width, SCROLL);
        self.mixer_scroll.resize(16, 120 + height, width, SCROLL);
        self.track_scroll
            .resize(16 + width, 120 + RULER, SCROLL, (split - RULER).max(1));
        self.pitch_scroll.resize(
            16 + width,
            120 + split + RULER,
            SCROLL,
            (height - split - RULER).max(1),
        );
        for (bar, visible) in [
            (&mut self.time_scroll, !model.mixer),
            (&mut self.mixer_scroll, model.mixer),
            (&mut self.track_scroll, model.track_view && !model.mixer),
            (&mut self.pitch_scroll, !model.tracks_only),
        ] {
            if visible {
                bar.show();
            } else {
                bar.hide();
            }
        }
        model.last_beat = model.song.end_tick().div_ceil(u64::from(model.song.ppq));
        self.sync_scrollbars(model);
        for tool in &mut self.piano_tools {
            if model.tracks_only {
                tool.hide()
            } else {
                tool.show()
            }
        }
        for tool in &mut self.navigation_tools {
            if model.mixer {
                tool.hide()
            } else {
                tool.show()
            }
        }
        self.toggle_editor.set_value(if model.mixer {
            3
        } else if model.tracks_only {
            1
        } else if model.track_view {
            0
        } else {
            2
        });
        if let Some(note) = model.selected {
            self.start.set_value(&note.start.to_string());
            self.key.set_value(&note.key.to_string());
            self.duration
                .set_value(&(note.end - note.start).to_string());
            self.velocity.set_value(&note.velocity.to_string());
        }
        model.chords = harmony::progression(&model.song);
        model.keys = model.song.key_map();
        self.update_music_display(model);
        self.grid.redraw();
        self.arrangement.redraw();
    }

    fn sync_scrollbars(&mut self, model: &Model) {
        let max = model
            .last_beat
            .saturating_sub(BEATS)
            .max(model.view_beat)
            .min(i32::MAX as u64 - BEATS);
        self.time_scroll.scroll_value(
            model.view_beat.min(max) as i32,
            BEATS as i32,
            0,
            (max + BEATS) as i32,
        );
        self.pitch_scroll
            .scroll_value(i32::from(127 - model.high_key), ROWS, 0, 128);
        let rows = track_view::visible_rows(&self.arrangement).min(model.song.tracks.len());
        let strips = mixer::visible_strips(&self.arrangement).min(model.song.tracks.len());
        let count = model.song.tracks.len();
        self.track_scroll.scroll_value(
            model.track_scroll.min(count - rows) as i32,
            rows as i32,
            0,
            count as i32,
        );
        self.mixer_scroll.scroll_value(
            model.mixer_scroll.min(count - strips) as i32,
            strips as i32,
            0,
            count as i32,
        );
    }

    fn update_music_display(&mut self, model: &Model) {
        let key = model
            .keys
            .get(
                model
                    .keys
                    .partition_point(|&(tick, _)| tick <= model.playhead)
                    .wrapping_sub(1),
            )
            .map(|entry| entry.1);
        let chord = model
            .chords
            .get(
                model
                    .chords
                    .partition_point(|&(tick, _)| tick <= model.playhead)
                    .wrapping_sub(1),
            )
            .and_then(|entry| entry.1)
            .map_or_else(
                || "—".to_owned(),
                |chord| chord.label(key.is_some_and(|k| k.0 < 0)),
            );
        let label = key.map_or_else(
            || chord.clone(),
            |key| format!("{chord} · {}", harmony::key_name(key)),
        );
        if self.chord.label() != label {
            self.chord.set_label(&label);
            self.chord.set_tooltip(&label);
        }
        let dock = if self.keyboard.docked() {
            if model.recording.is_some() {
                "Docked · REC"
            } else if model
                .playback
                .as_ref()
                .is_some_and(|playback| !playback.progress.paused.load(Ordering::Relaxed))
            {
                "Docked · PLAY"
            } else {
                "Piano docked"
            }
        } else {
            "Virtual piano"
        };
        if self.piano_dock.label() != dock {
            self.piano_dock.set_label(dock);
        }
    }

    fn advance_playhead(&mut self, model: &mut Model, micros: u64) {
        let tick = (model.tick_at_time)(micros);
        if tick != model.playhead {
            model.playhead = tick;
            let beat = tick / u64::from(model.song.ppq);
            if model.follow_playhead && (beat < model.view_beat || beat >= model.view_beat + BEATS)
            {
                model.view_beat = beat / BEATS * BEATS;
                self.beat.set_value(&(model.view_beat + 1).to_string());
                self.sync_scrollbars(model);
            }
            self.grid.redraw();
            self.arrangement.redraw();
        }
    }

    fn set_font(&mut self, model: &mut Model, path: &Path) -> song::Result<()> {
        let font = audio::load_font(path)?;
        self.presets.clear();
        self.preset.clear();
        let mut entries: Vec<_> = font.get_presets().iter().collect();
        entries.sort_by_key(|preset| (preset.get_bank_number(), preset.get_patch_number()));
        for preset in entries {
            let bank = preset.get_bank_number();
            let program = preset.get_patch_number();
            if !(0..=255).contains(&bank) || !(0..=127).contains(&program) {
                continue;
            }
            self.presets.push((bank, program));
            let name = menu_label(preset.get_name());
            self.preset
                .add(&format!("{bank:03} / {program:03}   {name}"));
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

fn roll_gutter(model: &Model) -> i32 {
    if model.track_view {
        track_view::GUTTER
    } else {
        LABEL
    }
}

fn grid_point(grid: &Frame, model: &Model, x: i32, y: i32) -> Option<GridPoint> {
    let x = x - grid.x() - roll_gutter(model);
    let y = y - grid.y() - RULER;
    let width = grid.w() - roll_gutter(model);
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
    let width = w - roll_gutter(model);
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
            roll_gutter(model),
            bottom - top,
            if black {
                BG
            } else {
                Color::from_rgb(48, 59, 78)
            },
        );
        draw::draw_rect_fill(
            x + roll_gutter(model),
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
            roll_gutter(model) - 5,
            bottom - top,
            Align::Inside | Align::Left,
        );
    }
    let next_step = model.step - first_tick % model.step;
    if model.step < u64::from(model.song.ppq) {
        draw::set_draw_color(Color::from_rgb(41, 52, 67));
        for offset in (next_step..shown_ticks).step_by(model.step as usize) {
            let line = x + roll_gutter(model) + (offset * width as u64 / shown_ticks) as i32;
            draw::draw_line(line, y + RULER, line, y + h);
        }
    }
    for beat in 0..BEATS {
        let line = x + roll_gutter(model) + (beat * width as u64 / BEATS) as i32;
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
            y + CHORD_BAND,
            48,
            RULER - CHORD_BAND,
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
            + roll_gutter(model)
            + ((note.start.max(first_tick) - first_tick) * width as u64 / shown_ticks) as i32;
        let right = x
            + roll_gutter(model)
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
    harmony::draw_ruler(grid, model, roll_gutter(model));
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

fn live_edit(model: &mut Model, messages: Vec<song::Packet>, rebuild: bool) {
    let Some(playback) = &model.playback else {
        return;
    };
    let old_position = model
        .offset
        .saturating_add(playback.progress.position.load(Ordering::Relaxed));
    let tick = (model.tick_at_time)(old_position);
    let end_tick = (model.tick_at_time)(model.duration_micros);
    let clock = model.song.tempo_clock();
    let time_for = |tick| {
        let mut lo = 0;
        // The tempo clock saturates, so long songs can use the full microsecond range.
        let mut hi = u64::MAX;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if clock(mid) < tick {
                lo = mid + 1
            } else {
                hi = mid
            }
        }
        lo
    };
    let mut timeline = model.song.timeline();
    // Keep transport running through silent sections, including when all tracks are muted.
    timeline.duration = timeline.duration.max(if rebuild {
        time_for(end_tick)
    } else {
        model.duration_micros
    });
    model.duration_micros = timeline.duration;
    if rebuild {
        model.offset = time_for(model.cue).min(timeline.duration.saturating_sub(1));
    }
    rhythm::append(&mut timeline, &model.song, model.click);
    timeline.seek(model.offset);
    if model.recording.is_some() && model.record_lead > 0 && model.offset == model.record_offset {
        rhythm::prepend_count_in(
            &mut timeline,
            model.count_beats,
            model.record_lead / u64::from(model.count_beats),
            model.count_sound,
        );
    }
    let position = time_for(tick)
        .saturating_sub(model.offset)
        .min(timeline.duration);
    let mut messages = messages;
    if rebuild && position > 0 {
        let mut chase = song::Timeline {
            instrument_tracks: vec![],
            buses: timeline.buses.clone(),
            packets: timeline
                .packets
                .iter()
                .copied()
                .take_while(|p| p.micros < position)
                .collect(),
            duration: position,
        };
        chase.seek(position);
        messages.extend(chase.packets);
    }
    model.tick_at_time = Box::new(clock);
    let _ = playback.edits.send(audio::PlaybackEdit {
        timeline,
        messages,
        reposition: rebuild.then_some(position),
    });
}

fn mixer_message(track: usize, channel: u8, controller: Option<u8>, value: u8) -> song::Packet {
    let (bytes, len) = match controller {
        Some(cc) => ([0xb0 | channel, cc, value], 3),
        None => ([0xc0 | channel, value, 0], 2),
    };
    song::Packet {
        track,
        micros: 0,
        bytes,
        len,
    }
}

fn tick_time(clock: &dyn Fn(u64) -> Tick, tick: Tick, duration: u64) -> u64 {
    let mut low = 0;
    let mut high = duration;
    while low < high {
        let mid = low + (high - low) / 2;
        if clock(mid) < tick {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low
}

fn halt_playback(model: &mut Model) {
    if let Some(playback) = model.playback.take() {
        model.jump_cue = None;
        let elapsed = playback.progress.position.load(Ordering::Relaxed);
        let position = model.offset.saturating_add(elapsed);
        // A cursor in the release tail resumes from the opening cue.
        if position < model.duration_micros {
            model.playhead = (model.tick_at_time)(position);
            model.cue = model.playhead;
        } else {
            model.cue = 0;
        }
    }
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
            Action::Stop
                | Action::RecordBacking
                | Action::ScrollTime(..)
                | Action::ScrollTracks(..)
                | Action::ScrollPitch(..)
                | Action::ScrollMixer(..)
                | Action::ResizeScrollbars
                | Action::VirtualKeyboard
                | Action::MetronomeChanged
                | Action::OutputChanged
                | Action::View
                | Action::Lane(..)
                | Action::ToggleEditor
        )
    {
        return Err("Press Stop to finish the MIDI take first.".into());
    }
    match action {
        Action::CountIn(beats) => {
            model.count_beats = beats;
            ui.status.set_label(if beats == 0 {
                "Count-in off"
            } else if beats == 4 {
                "Count-in: 1 bar"
            } else {
                "Count-in: 2 bars"
            });
        }
        Action::CountSound(sound) => {
            model.count_sound = sound;
            ui.status.set_label(&format!("Count-in sound: {sound:?}"));
        }
        Action::KeySignature => {
            let current = model
                .song
                .key_map()
                .into_iter()
                .rfind(|&(tick, _)| tick <= model.playhead)
                .map_or_else(|| "C major".to_string(), |(_, key)| harmony::key_name(key));
            if let Some(input) =
                dialog::input_default("Key at playhead (e.g. Bb major or F# minor)", &current)
            {
                let (name, mode) = input
                    .trim()
                    .split_once(' ')
                    .ok_or("Enter a note name followed by major or minor.")?;
                let minor = match mode.trim().to_ascii_lowercase().as_str() {
                    "major" => false,
                    "minor" => true,
                    _ => return Err("Use major or minor.".into()),
                };
                let names = if minor {
                    harmony::MINOR
                } else {
                    harmony::MAJOR
                };
                let index = names
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(name))
                    .ok_or("Use a standard MIDI key signature with up to seven sharps or flats.")?;
                model.checkpoint();
                model.song.set_key(model.playhead, (index as i8 - 7, minor));
            }
        }
        Action::Transpose => {
            if let Some(value) = dialog::input_default("Transpose song by semitones", "0") {
                let shift: i8 = value
                    .trim()
                    .parse()
                    .map_err(|_| "Enter a signed semitone count.")?;
                if shift != 0 {
                    let mut revised = model.song.clone();
                    revised.transpose(shift)?;
                    model.checkpoint();
                    model.song = revised;
                    model.selected = None;
                    model.fit_pitch();
                }
            }
        }
        Action::MetronomeChanged => {
            model.click = ui.metronome.value().then(|| {
                rhythm::Pattern::ALL[ui
                    .rhythm
                    .value()
                    .clamp(0, (rhythm::Pattern::ALL.len() - 1) as i32)
                    as usize]
            });
            if let Some(playback) = model
                .playback
                .as_ref()
                .filter(|p| !p.progress.done.load(Ordering::Relaxed))
            {
                // Only the isolated SoundFont stem gets immediate note-offs. On external MIDI,
                // percussion shares channel 10 with the song's own drums.
                let messages = if playback.is_software() {
                    rhythm::NOTES
                        .into_iter()
                        .map(|key| song::Packet {
                            track: model.song.tracks.len(),
                            micros: 0,
                            bytes: [0x89, key, 0],
                            len: 3,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                live_edit(model, messages, false);
            } else if model.recording.is_some() && model.click.is_some() {
                let micros = model.record_offset.saturating_add(
                    model
                        .recording
                        .as_ref()
                        .map_or(0, |take| take.elapsed())
                        .saturating_sub(model.record_lead),
                );
                model.cue = model.song.tempo_clock()(micros);
                model.jump_cue = Some(micros);
                apply(Action::RecordBacking, ui, model, background)?;
            }
            ui.status.set_label(if model.click.is_some() {
                "Metronome on"
            } else {
                "Metronome off"
            });
        }
        Action::TrackEffect(track, effect, amount) => {
            if let Some(track) = model.song.tracks.get(track) {
                track.bus.set_effect(effect, amount);
            }
        }
        Action::VirtualKeyboard => {
            let font = model
                .font
                .as_ref()
                .ok_or("Load an SF2 SoundFont to play the virtual piano.")?;
            let device = ui
                .audio_device
                .value()
                .checked_sub(1)
                .and_then(|index| model.sound_devices.get(index as usize));
            ui.keyboard.open(
                font,
                &model.song.tracks[model.track],
                model.effects.clone(),
                device,
            )?;
        }
        Action::BusValue(i, col, v) => {
            let bus = if i == usize::MAX {
                &model.effects.1
            } else {
                let Some(track) = model.song.tracks.get(i) else {
                    return Ok(true);
                };
                &track.bus
            };
            let [b, t, g, _] = bus.get();
            let mut bass = b as i8 - 12;
            let mut treble = t as i8 - 12;
            let mut gain = g;
            match col {
                8 => bass = v as i8,
                9 => treble = v as i8,
                _ => gain = v as u8,
            }
            bus.set(bass, treble, gain);
        }
        Action::MixPreset(master, bass, treble, gain) => {
            let bus = if master {
                &model.effects.1
            } else {
                &model.song.tracks[model.track].bus
            };
            bus.set(bass, treble, gain);
        }
        Action::Effects => {
            ui.effects.show();
        }
        Action::ToggleEditor => {
            model.mixer = ui.toggle_editor.value() == 3;
            model.tracks_only = model.mixer || ui.toggle_editor.value() == 1;
            model.track_view = ui.toggle_editor.value() != 2;
            ui.status.set_label(if model.mixer {
                "Drag volume or pan. Scroll to reach more tracks."
            } else if model.track_view {
                "Select a track or double-click a lane to edit its notes."
            } else {
                "Click to add or select a note • Drag to move • Right-click to delete"
            });
        }
        Action::Lane(index, edit_notes) => {
            if index < model.song.tracks.len() {
                model.track = index;
                model.selected = None;
                model.fit_pitch();
                if edit_notes {
                    model.track_view = false;
                    model.tracks_only = false;
                }
            }
        }
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
                let levels = Arc::new(model.effects.snapshot());
                let mut timeline = model.song.timeline();
                timeline.buses = timeline
                    .buses
                    .iter()
                    .map(|b| Arc::new(b.snapshot()))
                    .collect();
                let background = background.clone();
                std::thread::Builder::new()
                    .name("wav-export".into())
                    .spawn(move || {
                        let status = match audio::export_wav(&path, &font, timeline, levels) {
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
            model.track_scroll = model.track;
            model.mixer_scroll = model.track;
            model.selected = None;
        }
        Action::RemoveTrack => {
            model.checkpoint();
            model.song.remove_track(model.track);
            model.track = model.track.min(model.song.tracks.len() - 1);
            model.selected = None;
        }
        Action::FocusCell(index, col) => {
            model.column = col;
            return apply(Action::Lane(index, false), ui, model, background);
        }
        Action::MixerValue(index, col, _)
        | Action::Cell(index, col)
        | Action::Step(index, col, _) => {
            let track = &model.song.tracks[index];
            model.column = col;
            let values = track_view::values(index, track);
            let input = if let Action::MixerValue(_, _, v) = action {
                if values[col] == v.to_string() {
                    return Ok(true);
                }
                v.to_string()
            } else if let Action::Step(_, _, d) = action {
                (values[col].parse::<i16>()? + d)
                    .clamp(
                        if col == 2 { 1 } else { 0 },
                        if col == 2 { 16 } else { 127 },
                    )
                    .to_string()
            } else if col < 7 {
                let Some(text) = dialog::input_default(track_view::HEADERS[col], &values[col])
                else {
                    return Ok(true);
                };
                text
            } else {
                String::new()
            };
            let value = if (2..=6).contains(&col) {
                let n = input
                    .trim()
                    .parse::<u8>()
                    .map_err(|_| "Enter a MIDI value.")?;
                if n > 127 || (col == 2 && !(1..=16).contains(&n)) {
                    return Err("Use channel 1–16 or a MIDI value from 0–127.".into());
                }
                n
            } else {
                0
            };
            model.save_undo();
            model.track = index;
            model.selected = None;
            let track = &mut model.song.tracks[index];
            match col {
                1 => track.rename(input.trim()),
                2 => track.set_channel(value - 1),
                3 => track.set_setting(None, value),
                4 => track.set_setting(Some(0), value),
                5 => track.set_setting(Some(7), value),
                6 => track.set_setting(Some(10), value),
                7 => track.mute = !track.mute,
                8 => track.solo = !track.solo,
                _ => {}
            }
            let channel = track.channel;
            let messages = match col {
                3 => vec![mixer_message(index, channel, None, value)],
                4 => vec![
                    mixer_message(index, channel, Some(0), value),
                    mixer_message(index, channel, None, track.setting(None)),
                ],
                5 => vec![mixer_message(index, channel, Some(7), value)],
                6 => vec![mixer_message(index, channel, Some(10), value)],
                _ => Vec::new(),
            };
            if col != 1 {
                live_edit(model, messages, matches!(col, 2 | 7 | 8));
            }
            model.fit_pitch();
        }
        Action::BrowsePresets => {
            if model.font.is_none() {
                apply(Action::LoadFont, ui, model, background)?;
            }
            if model.font.is_some() {
                ui.preset_window.set_label(&format!(
                    "SoundFont presets · {}",
                    model.song.tracks[model.track].name()
                ));
                ui.preset_window.show();
            }
        }
        Action::TogglePlayback => {
            if let Some(playback) = model
                .playback
                .as_ref()
                .filter(|p| p.is_software() && !p.progress.done.load(Ordering::Relaxed))
            {
                let was_paused = playback.progress.paused.fetch_xor(true, Ordering::AcqRel);
                ui.status
                    .set_label(if was_paused { "Playing" } else { "Paused" });
            } else if model
                .playback
                .as_ref()
                .is_some_and(|p| !p.progress.done.load(Ordering::Relaxed))
            {
                halt_playback(model);
                ui.status.set_label("Paused");
            } else {
                return apply(Action::Play, ui, model, background);
            }
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
                model.save_undo();
                let track = &mut model.song.tracks[model.track];
                track.set_setting(Some(0), (bank % 128) as u8);
                track.set_setting(None, program as u8);
                model.selected = None;
                live_edit(
                    model,
                    vec![
                        mixer_message(model.track, channel, Some(0), (bank % 128) as u8),
                        mixer_message(model.track, channel, None, program as u8),
                    ],
                    false,
                );
            }
        }
        Action::TempoStep(d) => {
            ui.bpm.set_value(
                &(model.song.bpm() + f64::from(d))
                    .clamp(4., 999.)
                    .to_string(),
            );
            return apply(Action::Tempo, ui, model, background);
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
            model.save_undo();
            model.song.set_tempo(bpm);
            live_edit(model, Vec::new(), true);
            model.selected = None;
        }
        Action::ScrollTime(beat) => {
            model.follow_playhead = false;
            model.view_beat = beat;
        }
        Action::ScrollTracks(first) => model.track_scroll = first,
        Action::ScrollPitch(key) => model.high_key = key.clamp(ROWS as u8 - 1, 127),
        Action::ScrollMixer(first) => model.mixer_scroll = first,
        Action::ResizeScrollbars => {}
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
        Action::Jump(direction) => {
            let duration = model.song.timeline().duration;
            let clock = model.song.tempo_clock();
            let playback = model.playback.take();
            let running = playback.as_ref().is_some_and(|p| {
                !p.progress.paused.load(Ordering::Acquire)
                    && !p.progress.done.load(Ordering::Relaxed)
            });
            let position = playback
                .as_ref()
                .map_or_else(
                    || {
                        model
                            .jump_cue
                            .filter(|&time| clock(time) == model.playhead)
                            .unwrap_or_else(|| tick_time(&clock, model.playhead, duration))
                    },
                    |p| {
                        model
                            .offset
                            .saturating_add(p.progress.position.load(Ordering::Relaxed))
                    },
                )
                .min(duration);
            // Close the old stream or MIDI connection before opening a replacement.
            drop(playback);
            let target = position
                .saturating_add_signed(i64::from(direction) * 5_000_000)
                .min(duration);
            model.cue = clock(target);
            model.jump_cue = Some(target);
            model.playhead = model.cue;
            let beat = model.cue / u64::from(model.song.ppq);
            if beat < model.view_beat || beat >= model.view_beat.saturating_add(BEATS) {
                model.view_beat = beat / BEATS * BEATS;
            }
            let tenths = target / 100_000;
            ui.clock.set_label(&format!(
                "{:02}:{:02}.{}",
                tenths / 600,
                tenths / 10 % 60,
                tenths % 10
            ));
            ui.last_stop = None;
            if running && target < duration {
                return apply(Action::Play, ui, model, background);
            }
            if running {
                ui.status.set_label("Playback finished.");
            }
        }
        Action::Seek(tick) => {
            model.follow_playhead = true;
            let playing = model.playback.take().is_some();
            model.cue = tick;
            model.jump_cue = None;
            model.playhead = tick;
            if playing {
                return apply(Action::Play, ui, model, background);
            }
        }
        Action::Play | Action::RecordBacking => {
            model.follow_playhead = true;
            ui.last_stop = None;
            if let Some(playback) = &model.playback {
                if playback.progress.paused.swap(false, Ordering::AcqRel) {
                    ui.status.set_label("Playing");
                    return Ok(true);
                }
            }
            let mut timeline = model.song.timeline();
            if model.recording.is_some() && (ui.metronome.value() || model.record_lead > 0) {
                timeline.duration = timeline
                    .duration
                    .max(model.record_offset.saturating_add(3_600_000_000));
            }
            model.click = ui.metronome.value().then(|| {
                rhythm::Pattern::ALL[ui
                    .rhythm
                    .value()
                    .clamp(0, (rhythm::Pattern::ALL.len() - 1) as i32)
                    as usize]
            });
            if model.click.is_some() && timeline.duration == 0 {
                let clock = model.song.tempo_clock();
                timeline.duration = tick_time(&clock, u64::from(model.song.ppq) * 16, u64::MAX);
            }
            rhythm::append(&mut timeline, &model.song, model.click);
            if (timeline.packets.is_empty() && model.record_lead == 0) || timeline.duration == 0 {
                return Err("Add notes to an audible track before playing.".into());
            }
            model.playback = None;
            model.tick_at_time = Box::new(model.song.tempo_clock());
            model.duration_micros = timeline.duration;
            let low = model
                .jump_cue
                .filter(|&time| (model.tick_at_time)(time) == model.cue)
                .unwrap_or_else(|| tick_time(&model.tick_at_time, model.cue, timeline.duration));
            if low >= timeline.duration {
                return Err("Choose a beat before the end of the song.".into());
            }
            model.offset = low;
            timeline.seek(low);
            if model.recording.is_some() && model.record_lead > 0 && low == model.record_offset {
                rhythm::prepend_count_in(
                    &mut timeline,
                    model.count_beats,
                    model.record_lead / u64::from(model.count_beats),
                    model.count_sound,
                );
            }
            let looping = ui.looping.value() && model.recording.is_none();
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
                audio::Playback::software(font, timeline, looping, device, model.effects.clone())?
            });
            model.playhead = model.cue;
            ui.status.set_label("Playing");
        }
        Action::Record | Action::RecordSelected => {
            let into_track = matches!(action, Action::RecordSelected);
            let port = ui
                .midi_input
                .value()
                .checked_sub(1)
                .and_then(|index| model.inputs.get(index as usize));
            let target = into_track.then_some(model.track);
            let clock = model.song.tempo_clock();
            let duration = model.song.timeline().duration;
            let offset = if into_track {
                tick_time(&clock, model.playhead, duration)
            } else {
                0
            };
            let record_tick = clock(offset);
            let lead = model.song.beat_micros_at(record_tick) * u64::from(model.count_beats);
            let recording = if let Some(port) = port {
                recording::Recording::start(&port.port)?
            } else {
                apply(Action::VirtualKeyboard, ui, model, background)?;
                let (recording, capture) = recording::Recording::virtual_piano();
                ui.keyboard.capture(Some(capture));
                recording
            };
            model.playback = None;
            model.captured.clear();
            model.record_target = target;
            model.record_offset = offset;
            model.record_lead = lead;
            model.cue = clock(offset);
            model.jump_cue = Some(offset);
            model.playhead = model.cue;
            model.tick_at_time = Box::new(clock);
            model.recording = Some(recording);
            if lead > 0
                || ui.metronome.value()
                || into_track
                    && offset < duration
                    && (model.font.is_some() || ui.output.value() > 0)
            {
                // A failed backing output must not discard the recording.
                if let Err(error) = apply(Action::RecordBacking, ui, model, background) {
                    model.record_lead = 0;
                    ui.status
                        .set_label(&format!("Recording without backing audio · {error}"));
                    return Ok(true);
                }
            }
            ui.status.set_label(if into_track {
                "Recording into selected track · Stop to keep the take"
            } else {
                "Recording new take · Stop to keep it"
            });
        }
        Action::OutputChanged => {
            if model.playback.take().is_some() {
                ui.clock.set_label("00:00.0");
                ui.status
                    .set_label("Playback stopped after changing the output.");
            }
        }
        Action::Stop => {
            let now = std::time::Instant::now();
            let rewind = ui
                .last_stop
                .take()
                .is_some_and(|last| now.duration_since(last).as_millis() <= 400);
            if rewind {
                model.playback = None;
                model.playhead = 0;
                model.cue = 0;
                model.jump_cue = None;
                model.view_beat = 0;
                ui.clock.set_label("00:00.0");
            } else {
                halt_playback(model);
                ui.last_stop = Some(now);
            }
            if let Some(recording) = model.recording.take() {
                ui.keyboard.capture(None);
                model.playback = None;
                let (duration, mut events, dropped) =
                    recording.finish(std::mem::take(&mut model.captured));
                let lead = std::mem::take(&mut model.record_lead);
                let mut recorded_keys = [[0_u32; 128]; 16];
                events.retain_mut(|(time, [status, key, velocity])| {
                    let Some(relative) = time.checked_sub(lead) else {
                        return false;
                    };
                    *time = relative;
                    if lead > 0 && matches!(*status & 0xf0, 0x80 | 0x90) {
                        let count =
                            &mut recorded_keys[usize::from(*status & 15)][usize::from(*key)];
                        if *status & 0xf0 == 0x90 && *velocity != 0 {
                            *count += 1;
                        } else if *count == 0 {
                            // A key pressed before the count ended may be released during the take.
                            return false;
                        } else {
                            *count -= 1;
                        }
                    }
                    true
                });
                let duration = duration.saturating_sub(lead);
                if events.is_empty() {
                    ui.status.set_label("The MIDI take was empty.");
                } else {
                    model.checkpoint();
                    model.track = model.song.add_recording(
                        duration,
                        events,
                        model.record_offset,
                        model.record_target.take(),
                    );
                    model.fit_pitch();
                    model.selected = None;
                    model.view_beat = 0;
                    if dropped == 0 {
                        ui.status.set_label("MIDI take added.");
                    } else {
                        ui.status.set_label(&format!(
                            "Take added. Input queue overflow dropped {dropped} MIDI messages."
                        ));
                    }
                }
                ui.clock.set_label("00:00.0");
                return Ok(true);
            }
            ui.status.set_label(if rewind {
                "Returned to beginning"
            } else {
                "Stopped"
            });
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

struct SpaceShortcut {
    window: usize,
    text_fields: [usize; 7],
    sender: app::Sender<Action>,
    held: bool,
}

thread_local! {
    static SPACE_SHORTCUT: RefCell<Option<SpaceShortcut>> = const { RefCell::new(None) };
}

fn install_space_shortcut(ui: &Ui, sender: app::Sender<Action>) {
    SPACE_SHORTCUT.with(|slot| {
        *slot.borrow_mut() = Some(SpaceShortcut {
            window: ui.win.as_widget_ptr() as usize,
            text_fields: [
                ui.bpm.as_widget_ptr(),
                ui.beat.as_widget_ptr(),
                ui.pitch.as_widget_ptr(),
                ui.start.as_widget_ptr(),
                ui.key.as_widget_ptr(),
                ui.duration.as_widget_ptr(),
                ui.velocity.as_widget_ptr(),
            ]
            .map(|ptr| ptr as usize),
            sender,
            held: false,
        });
    });
    // Intercept transport Space before FLTK sends it to the focused button.
    // Forward all other events with the original window pointer supplied by FLTK.
    unsafe {
        app::event_dispatch(|event, window| {
            let consumed = SPACE_SHORTCUT.with(|slot| {
                let mut slot = slot.borrow_mut();
                let Some(shortcut) = slot.as_mut() else {
                    return false;
                };
                if event == Event::KeyUp && app::event_key() == Key::from_char(' ') {
                    return std::mem::take(&mut shortcut.held);
                }
                if window as usize != shortcut.window {
                    return false;
                }
                if event == Event::Unfocus {
                    shortcut.held = false;
                }
                let modifiers = app::event_state();
                if event == Event::KeyDown
                    && modifiers.contains(Shortcut::Ctrl)
                    && !modifiers.intersects(Shortcut::Alt | Shortcut::Meta | Shortcut::Shift)
                {
                    let direction = match app::event_key() {
                        Key::Left => -1,
                        Key::Right => 1,
                        _ => 0,
                    };
                    if direction != 0 {
                        shortcut.sender.send(Action::Jump(direction));
                        return true;
                    }
                }
                let modified =
                    app::event_state().intersects(Shortcut::Ctrl | Shortcut::Alt | Shortcut::Meta);
                if event != Event::KeyDown || app::event_key() != Key::from_char(' ') || modified {
                    return false;
                }
                if app::focus()
                    .is_some_and(|w| shortcut.text_fields.contains(&(w.as_widget_ptr() as usize)))
                {
                    return false;
                }
                if !shortcut.held {
                    shortcut.held = true;
                    shortcut.sender.send(Action::TogglePlayback);
                }
                true
            });
            consumed || app::handle_raw(event, window)
        });
    }
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
    install_space_shortcut(&ui, send);
    ui.win.show();
    while ui.win.shown() {
        let _ = app::wait_for(0.05);
        while let Some(action) = receive.recv() {
            // Native dialogs can dispatch window events. Grid drawing uses try_borrow while an action runs.
            let mut model = model.borrow_mut();
            match apply(action, &mut ui, &mut model, &background) {
                Ok(false) => return,
                Ok(true) => match action {
                    Action::ScrollTime(..)
                    | Action::ScrollTracks(..)
                    | Action::ScrollPitch(..)
                    | Action::ScrollMixer(..)
                    | Action::ResizeScrollbars => {
                        ui.beat.set_value(&(model.view_beat + 1).to_string());
                        ui.pitch.set_value(&model.high_key.to_string());
                        ui.sync_scrollbars(&model);
                        ui.grid.redraw();
                        ui.arrangement.redraw();
                    }
                    _ => ui.sync(&mut model),
                },
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
                let recorded = recording.elapsed();
                let remaining = model.record_lead.saturating_sub(recorded);
                if remaining > 0 {
                    let interval = model.record_lead / u64::from(model.count_beats);
                    ui.status
                        .set_label(&format!("Count-in · {}", remaining.div_ceil(interval)));
                } else if ui.status.label().starts_with("Count-in ·") {
                    ui.status.set_label("Recording · Stop to keep the take");
                }
                let micros = model
                    .record_offset
                    .saturating_add(recorded.saturating_sub(model.record_lead));
                let elapsed = micros / 100_000;
                ui.advance_playhead(model, micros);
                ui.clock.set_label(&format!(
                    "{:02}:{:02}.{}",
                    elapsed / 600,
                    elapsed / 10 % 60,
                    elapsed % 10
                ));
            }
        } else if let Some(playback) = &model.playback {
            let micros = playback
                .progress
                .position
                .load(Ordering::Relaxed)
                .saturating_add(model.offset)
                .min(model.duration_micros);
            let elapsed = micros / 100_000;
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
            ui.advance_playhead(&mut model, micros);
            // Audio levels continue changing after the transport reaches the final tick.
            if model.mixer {
                ui.arrangement.redraw();
            }
        }
        ui.update_music_display(&model);
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
            audio::export_wav(Path::new(output), &font, song.timeline(), Arc::default())
        }
        _ => Err("Invalid command. Run with --help for usage.".into()),
    }
}
