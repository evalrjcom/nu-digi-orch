use super::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Pencil,
    Select,
    Velocity,
    Tone,
    Chord,
    Inversion,
    ArpUp,
    ArpDown,
}

impl Tool {
    pub const OPTIONS: [Self; 8] = [
        Self::Pencil,
        Self::Select,
        Self::Velocity,
        Self::Tone,
        Self::Chord,
        Self::Inversion,
        Self::ArpUp,
        Self::ArpDown,
    ];
}

#[derive(Clone, Copy)]
enum Mode {
    Move,
    LeftEdge,
    RightEdge,
    Velocity,
    Empty(Tool),
}

#[derive(Clone, Copy)]
pub struct Gesture {
    track: usize,
    origin: GridPoint,
    target: Option<Note>,
    mode: Mode,
    additive: bool,
    y: i32,
}

impl Gesture {
    fn new(frame: &Frame, model: &Model, origin: GridPoint) -> Self {
        let chord_tool = matches!(
            model.roll_tool,
            Tool::Tone | Tool::Chord | Tool::Inversion | Tool::ArpUp | Tool::ArpDown
        );
        let target = (!chord_tool).then(|| model.hit(origin)).flatten();
        let mode = match target {
            None => Mode::Empty(model.roll_tool),
            Some(_) if model.roll_tool == Tool::Velocity => Mode::Velocity,
            Some(note) => {
                let left = tick_x(frame, model, note.start);
                let right = tick_x(frame, model, note.end);
                let x = app::event_x();
                let first = model.view_beat * u64::from(model.song.ppq);
                let last = first + BEATS * u64::from(model.song.ppq);
                let grip = ((right - left) / 4).min(6);
                if grip >= 2 && note.end <= last && (right - x).abs() <= grip {
                    Mode::RightEdge
                } else if grip >= 2 && note.start >= first && (x - left).abs() <= grip {
                    Mode::LeftEdge
                } else {
                    Mode::Move
                }
            }
        };
        Self {
            track: model.track,
            origin,
            target,
            mode,
            additive: app::event_state().contains(Shortcut::Shift),
            y: app::event_y(),
        }
    }

    fn transform(self, model: &mut Model, end: GridPoint, dy: i32) -> song::Result<()> {
        let Some(target) = self.target.filter(|note| model.notes.contains(note)) else {
            return Ok(());
        };
        if self.additive && dy == 0 && self.origin.tick == end.tick && self.origin.key == end.key {
            if model.selected.contains(&target) {
                model.selected.retain(|note| *note != target);
            } else {
                model.selected.push(target);
            }
            return Ok(());
        }
        let (originals, changed) = self.transformed(model, target, end, dy);
        update(model, originals, changed)
    }

    fn transformed(
        self,
        model: &Model,
        target: Note,
        end: GridPoint,
        dy: i32,
    ) -> (Vec<Note>, Vec<Note>) {
        let mut group = if model.selected.contains(&target) || self.additive {
            model.selected.clone()
        } else {
            Vec::new()
        };
        if !group.contains(&target) {
            group.push(target);
        }
        let step = model.step;
        let delta = signed_delta(end.tick, self.origin.tick);
        let pitch = i16::from(end.key) - i16::from(self.origin.key);
        let change = match self.mode {
            Mode::Move => {
                let min = group.iter().map(|n| n.start).min().unwrap_or(0);
                let low = group.iter().map(|n| n.key).min().unwrap_or(0);
                let high = group.iter().map(|n| n.key).max().unwrap_or(127);
                let delta = if delta == 0 {
                    0
                } else {
                    signed_delta(
                        snap(target.start.saturating_add_signed(delta), step),
                        target.start,
                    )
                    .max(-(min.min(i64::MAX as u64) as i64))
                };
                let pitch = pitch.clamp(-i16::from(low), 127 - i16::from(high));
                group
                    .iter()
                    .map(|&n| Note {
                        start: n.start.saturating_add_signed(delta),
                        end: n.end.saturating_add_signed(delta),
                        key: (i16::from(n.key) + pitch) as u8,
                        ..n
                    })
                    .collect()
            }
            Mode::RightEdge | Mode::LeftEdge => {
                let right = matches!(self.mode, Mode::RightEdge);
                let edge = if right { target.end } else { target.start };
                let amount = if delta == 0 {
                    0
                } else {
                    signed_delta(snap(edge.saturating_add_signed(delta), step), edge)
                };
                group
                    .iter()
                    .map(|&n| {
                        if right {
                            Note {
                                end: n
                                    .end
                                    .saturating_add_signed(amount)
                                    .max(n.start.saturating_add(1)),
                                ..n
                            }
                        } else {
                            Note {
                                start: n
                                    .start
                                    .saturating_add_signed(amount)
                                    .min(n.end.saturating_sub(1)),
                                ..n
                            }
                        }
                    })
                    .collect()
            }
            Mode::Velocity => group
                .iter()
                .map(|&n| Note {
                    velocity: (i32::from(n.velocity) + dy).clamp(1, 127) as u8,
                    ..n
                })
                .collect(),
            Mode::Empty(_) => unreachable!("a note gesture has a target"),
        };
        (group, change)
    }
}

fn signed_delta(a: Tick, b: Tick) -> i64 {
    (i128::from(a) - i128::from(b)).clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

pub fn snap(tick: Tick, step: Tick) -> Tick {
    tick.saturating_add(step / 2) / step * step
}

fn tick_x(frame: &Frame, model: &Model, tick: Tick) -> i32 {
    let left = frame.x() + roll_gutter(model);
    let width = frame.w() - roll_gutter(model);
    let first = model.view_beat * u64::from(model.song.ppq);
    let span = BEATS * u64::from(model.song.ppq);
    left + ((tick.saturating_sub(first).min(span) * width as u64) / span) as i32
}

type Pointer = (i32, i32);

#[derive(Clone, Copy)]
struct Drag {
    gesture: Gesture,
    start: Pointer,
    end: Pointer,
}

pub fn install(frame: &mut Frame, model: &Rc<RefCell<Model>>, tx: app::Sender<Action>) {
    let drag = Rc::new(RefCell::new(None::<Drag>));
    let preview = Rc::clone(&drag);
    let data = Rc::clone(model);
    frame.draw(move |f| {
        if let Ok(m) = data.try_borrow() {
            draw_grid(f, &m);
            if let Some(drag) = *preview.borrow() {
                if drag.gesture.track == m.track && drag.start != drag.end {
                    draw_drag(f, &m, drag);
                }
            }
            track_view::draw_playhead(f, &m, roll_gutter(&m));
        }
    });
    let data = Rc::clone(model);
    frame.handle(move |f, event| {
        let Ok(m) = data.try_borrow() else {
            return false;
        };
        if drag.borrow().is_none() && track_view::seek_ruler(f, &m, event, roll_gutter(&m), tx) {
            return true;
        }
        let (x, y) = (app::event_x(), app::event_y());
        match event {
            Event::Push => {
                let Some(origin) = grid_point(f, &m, x, y) else {
                    return false;
                };
                let _ = f.take_focus();
                match app::event_mouse_button() {
                    MouseButton::Right => tx.send(Action::EraseNote(origin)),
                    MouseButton::Left => {
                        *drag.borrow_mut() = Some(Drag {
                            gesture: Gesture::new(f, &m, origin),
                            start: (x, y),
                            end: (x, y),
                        })
                    }
                    _ => return false,
                }
            }
            Event::Drag => {
                let mut drag = drag.borrow_mut();
                let Some(active) = drag.as_mut() else {
                    return false;
                };
                active.end = (x, y);
                f.redraw();
            }
            Event::Released => {
                let Some(active) = drag.borrow_mut().take() else {
                    return false;
                };
                f.redraw();
                let small_move = active.start.0.abs_diff(x).max(active.start.1.abs_diff(y)) < 3;
                if let Some(end) = drag_point(f, &m, active.gesture.origin, (x, y), small_move) {
                    let dy = if small_move { 0 } else { active.gesture.y - y };
                    tx.send(Action::RollGesture(active.gesture, end, dy));
                }
            }
            Event::MouseWheel => {
                if drag.borrow().is_none() {
                    let key = i32::from(m.high_key) - app::event_dy_value();
                    tx.send(Action::ScrollPitch(key.clamp(ROWS - 1, 127) as u8));
                }
            }
            Event::Unfocus => {
                drag.borrow_mut().take();
                f.redraw();
            }
            Event::Focus => {}
            Event::KeyDown if app::event_key() == Key::Escape => {
                drag.borrow_mut().take();
                f.redraw();
            }
            Event::KeyDown if app::event_key() == Key::Delete => tx.send(Action::DeleteNote),
            _ => return false,
        }
        true
    });
}

fn drag_point(
    f: &Frame,
    model: &Model,
    origin: GridPoint,
    end: Pointer,
    click: bool,
) -> Option<GridPoint> {
    if click {
        return Some(origin);
    }
    grid_point(
        f,
        model,
        end.0.clamp(f.x() + roll_gutter(model), f.x() + f.w() - 1),
        end.1.clamp(f.y() + RULER, f.y() + f.h() - 1),
    )
}

fn draw_drag(f: &Frame, m: &Model, drag: Drag) {
    let (x, y) = drag.start;
    let (end_x, end_y) = drag.end;
    let left = f.x() + roll_gutter(m);
    let top = f.y() + RULER;
    draw::push_clip(left, top, f.x() + f.w() - left, f.y() + f.h() - top);
    let color = Color::from_rgb(255, 204, 111);
    draw::set_draw_color(color);
    match drag.gesture.mode {
        Mode::Empty(Tool::Select) => draw::draw_rect(
            x.min(end_x),
            y.min(end_y),
            x.abs_diff(end_x).max(1) as i32,
            y.abs_diff(end_y).max(1) as i32,
        ),
        Mode::Empty(Tool::Pencil) => draw::draw_line(x, y, end_x, y),
        Mode::Move | Mode::LeftEdge | Mode::RightEdge | Mode::Velocity => {
            if let (Some(target), Some(end)) = (
                drag.gesture.target,
                drag_point(f, m, drag.gesture.origin, drag.end, false),
            ) {
                let (_, changed) = drag.gesture.transformed(m, target, end, y - end_y);
                let height = f.h() - RULER;
                for note in changed {
                    let row = i32::from(m.high_key) - i32::from(note.key);
                    if !(0..ROWS).contains(&row) {
                        continue;
                    }
                    let x = tick_x(f, m, note.start);
                    let w = tick_x(f, m, note.end) - x;
                    if w <= 0 {
                        continue;
                    }
                    let y = top + row * height / ROWS;
                    let h = top + (row + 1) * height / ROWS - y;
                    draw::draw_rect(x, y + 1, w, (h - 2).max(1));
                    if matches!(drag.gesture.mode, Mode::Velocity) && h > 4 {
                        draw::draw_rect_fill(
                            x,
                            y + h - 4,
                            (w * i32::from(note.velocity) / 127).max(1),
                            2,
                            color,
                        );
                    }
                }
            }
        }
        Mode::Empty(_) => {}
    }
    draw::pop_clip();
}

pub fn gesture(
    model: &mut Model,
    g: Gesture,
    end: GridPoint,
    dy: i32,
    length: Tick,
    velocity: u8,
) -> song::Result<()> {
    if model.track != g.track || model.tracks_only {
        return Ok(());
    }
    let Mode::Empty(tool) = g.mode else {
        return g.transform(model, end, dy);
    };
    let origin = g.origin;
    match tool {
        Tool::Select => {
            let start = origin.tick.min(end.tick);
            let finish = origin.tick.max(end.tick);
            let pitches = origin.key.min(end.key)..=origin.key.max(end.key);
            if !g.additive {
                model.selected.clear();
            }
            if start != finish || origin.key != end.key {
                let mut selected: HashSet<_> = model.selected.iter().copied().collect();
                model
                    .selected
                    .extend(model.notes.iter().copied().filter(|note| {
                        note.start <= finish
                            && note.end > start
                            && pitches.contains(&note.key)
                            && selected.insert(*note)
                    }));
            }
        }
        Tool::Pencil => {
            let step = model.step;
            let start = origin.tick.min(end.tick) / step * step;
            let finish = if origin.tick.abs_diff(end.tick) >= step {
                snap(origin.tick.max(end.tick), step).max(start.saturating_add(step))
            } else {
                start.saturating_add(length)
            };
            let channel = model.song.tracks[model.track].channel;
            add(
                model,
                vec![new_note(start, finish, origin.key, velocity, channel)],
            )?;
        }
        Tool::Velocity => {}
        _ => {
            let notes = chord_notes(model, tool, origin, length, velocity)?;
            add(model, notes)?;
        }
    }
    Ok(())
}

fn new_note(start: Tick, end: Tick, key: u8, velocity: u8, channel: u8) -> Note {
    Note {
        on: 0,
        off: 0,
        start,
        end,
        key,
        velocity,
        channel,
    }
}

fn chord_notes(
    model: &Model,
    tool: Tool,
    point: GridPoint,
    length: Tick,
    velocity: u8,
) -> song::Result<Vec<Note>> {
    let channel = model.song.tracks[model.track].channel;
    if channel == 9 {
        return Err("Choose a melodic track for the chord tools.".into());
    }
    let index = model
        .chords
        .partition_point(|&(tick, _)| tick <= point.tick);
    let (chord_start, chord) = index
        .checked_sub(1)
        .and_then(|i| {
            let (tick, chord) = model.chords[i];
            chord.map(|chord| (tick, chord))
        })
        .ok_or("Click under a detected chord in the piano roll.")?;
    let start = (point.tick / model.step * model.step).max(chord_start);
    let limit = model.chords.get(index).map_or(Tick::MAX, |&(tick, _)| tick);
    let keys = chord.voicing(point.key, tool == Tool::Inversion);
    let mut notes = Vec::new();
    let finish = start.saturating_add(length).min(limit);
    match tool {
        Tool::Tone => notes.push(new_note(
            start,
            finish,
            chord.closest_tone(point.key),
            velocity,
            channel,
        )),
        Tool::Chord | Tool::Inversion => notes.extend(
            keys.into_iter()
                .map(|key| new_note(start, finish, key, velocity, channel)),
        ),
        Tool::ArpUp | Tool::ArpDown => {
            let mut keys = keys;
            if tool == Tool::ArpDown {
                keys.reverse();
            }
            for (i, key) in keys.into_iter().enumerate() {
                let at = start.saturating_add(model.step.saturating_mul(i as Tick));
                if at >= limit {
                    break;
                }
                notes.push(new_note(
                    at,
                    at.saturating_add(length.min(model.step)).min(limit),
                    key,
                    velocity,
                    channel,
                ));
            }
        }
        _ => unreachable!("only chord tools generate harmony"),
    }
    Ok(notes)
}

fn validate(existing: &[Note], originals: &[Note], new: &[Note]) -> song::Result<()> {
    if new
        .iter()
        .any(|n| n.start >= n.end || n.end > i64::MAX as u64)
    {
        return Err("A note must have a positive length and a valid end tick.".into());
    }
    let removed: HashSet<_> = originals.iter().map(|n| n.on).collect();
    let mut intervals: Vec<_> = existing
        .iter()
        .filter(|n| !removed.contains(&n.on))
        .map(|n| (n.channel, n.key, n.start, n.end, false))
        .chain(new.iter().map(|n| (n.channel, n.key, n.start, n.end, true)))
        .collect();
    intervals.sort_unstable();
    let mut previous_pitch = None;
    let (mut tail, mut changed_tail) = (0, 0);
    for (channel, key, start, end, changed) in intervals {
        if previous_pitch != Some((channel, key)) {
            previous_pitch = Some((channel, key));
            (tail, changed_tail) = (0, 0);
        }
        if start < if changed { tail } else { changed_tail } {
            return Err("This edit would overlap notes of the same pitch and MIDI channel.".into());
        }
        tail = tail.max(end);
        if changed {
            changed_tail = changed_tail.max(end);
        }
    }
    Ok(())
}

pub fn update(model: &mut Model, originals: Vec<Note>, mut changed: Vec<Note>) -> song::Result<()> {
    let timing_changed: Vec<_> = originals
        .iter()
        .zip(&changed)
        .filter(|(a, b)| (a.start, a.end, a.key) != (b.start, b.end, b.key))
        .collect();
    let retimed = !timing_changed.is_empty();
    if retimed {
        let removed: Vec<_> = timing_changed.iter().map(|(a, _)| **a).collect();
        let added: Vec<_> = timing_changed.iter().map(|(_, b)| **b).collect();
        validate(&model.notes, &removed, &added)?;
    }
    if originals != changed {
        model.checkpoint();
        let track = &mut model.song.tracks[model.track];
        for (&before, &after) in originals.iter().zip(&changed) {
            if before != after {
                track.change_note(after);
            }
        }
        if retimed {
            track.order_retriggers(&mut changed);
        }
    }
    model.selected = changed;
    Ok(())
}

pub fn add(model: &mut Model, notes: Vec<Note>) -> song::Result<()> {
    validate(&model.notes, &[], &notes)?;
    if !notes.is_empty() {
        model.checkpoint();
        let track = &mut model.song.tracks[model.track];
        model.selected = notes
            .iter()
            .map(|n| track.add_note(n.start, n.end, n.key, n.velocity))
            .collect();
        track.order_retriggers(&mut model.selected);
    }
    Ok(())
}

pub fn remove(model: &mut Model, notes: &[Note]) {
    if !notes.is_empty() {
        model.checkpoint();
        model.song.tracks[model.track].delete_notes(notes);
        model.selected.clear();
    }
}

pub fn paste(model: &mut Model, notes: &[Note], at: Tick) -> song::Result<()> {
    let Some(first) = notes.iter().map(|n| n.start).min() else {
        return Ok(());
    };
    let channel = model.song.tracks[model.track].channel;
    let moved = notes
        .iter()
        .map(|n| {
            new_note(
                at.saturating_add(n.start - first),
                at.saturating_add(n.end - first),
                n.key,
                n.velocity,
                channel,
            )
        })
        .collect();
    add(model, moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note_model() -> Model {
        let mut model = Model::new();
        let track = &mut model.song.tracks[0];
        track.add_note(120, 240, 60, 90);
        track.add_note(180, 300, 64, 100);
        track.add_note(360, 480, 60, 96);
        model.notes = track.notes();
        model
    }

    #[test]
    fn group_move_preserves_offsets_and_failed_edits_are_atomic() {
        let mut model = note_model();
        model.selected = model.notes[..2].to_vec();
        let gesture = Gesture {
            track: 0,
            origin: GridPoint { tick: 120, key: 60 },
            target: Some(model.notes[0]),
            mode: Mode::Move,
            additive: false,
            y: 0,
        };
        gesture
            .transform(&mut model, GridPoint { tick: 240, key: 62 }, 0)
            .unwrap();
        assert_eq!(
            model
                .selected
                .iter()
                .map(|n| (n.start, n.key))
                .collect::<Vec<_>>(),
            [(240, 62), (300, 66)]
        );
        assert_eq!(model.undo.len(), 1);
        model.notes = model.song.tracks[0].notes();
        let events = model.song.tracks[0].notes();
        let result = add(
            &mut model,
            vec![new_note(365, 400, 60, 96, 0), new_note(360, 410, 67, 96, 0)],
        );
        assert!(result.is_err());
        assert_eq!(model.undo.len(), 1);
        assert_eq!(model.song.tracks[0].notes(), events);
        let selected = model.selected.clone();
        remove(&mut model, &selected);
        assert_eq!(model.song.tracks[0].notes().len(), 1);
        model.history(false);
        assert_eq!(model.song.tracks[0].notes(), events);
    }

    #[test]
    fn pasted_note_releases_before_an_existing_same_key_attack() {
        let mut model = Model::new();
        model.song.tracks[0].add_note(120, 240, 60, 90);
        model.notes = model.song.tracks[0].notes();
        add(&mut model, vec![new_note(0, 120, 60, 100, 0)]).unwrap();
        let notes = model.song.tracks[0].notes();
        assert_eq!(notes[0], model.selected[0]);
        let packets = model.song.timeline().packets;
        let at_boundary: Vec<_> = packets
            .iter()
            .filter(|p| p.micros == 125_000)
            .map(|p| p.bytes[0])
            .collect();
        assert_eq!(at_boundary, [0x80, 0x90]);
    }

    #[test]
    fn chord_tools_use_detected_quality_snap_and_next_harmonic_boundary() {
        let mut model = Model::new();
        let track = &mut model.song.tracks[0];
        for key in [60, 63, 67, 70] {
            track.add_note(0, 330, key, 96);
        }
        model.chords = harmony::progression(&model.song);
        model.song.add_track();
        model.track = 1;
        let point = GridPoint { tick: 31, key: 72 };
        let arpeggio = chord_notes(&model, Tool::ArpUp, point, 200, 85).unwrap();
        assert_eq!(
            arpeggio
                .iter()
                .map(|n| (n.start, n.end, n.key, n.velocity))
                .collect::<Vec<_>>(),
            [(0, 120, 72, 85), (120, 240, 75, 85), (240, 330, 79, 85)]
        );
        let inversion = chord_notes(&model, Tool::Inversion, point, 120, 96).unwrap();
        assert_eq!(
            inversion.iter().map(|n| n.key % 12).collect::<Vec<_>>(),
            [3, 7, 10, 0]
        );
        let pitch = chord_notes(
            &model,
            Tool::Tone,
            GridPoint { tick: 31, key: 127 },
            120,
            96,
        )
        .unwrap()[0]
            .key;
        assert_eq!(pitch, 127);
        assert!(chord_notes(
            &model,
            Tool::Chord,
            GridPoint { tick: 400, key: 72 },
            120,
            96
        )
        .is_err());
    }
}
