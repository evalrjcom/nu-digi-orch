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
    Motif,
}

impl Tool {
    pub const OPTIONS: [Self; 9] = [
        Self::Pencil,
        Self::Select,
        Self::Velocity,
        Self::Tone,
        Self::Chord,
        Self::Inversion,
        Self::ArpUp,
        Self::ArpDown,
        Self::Motif,
    ];
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lane {
    #[default]
    Hidden,
    Velocity,
    Expression,
    Sustain,
}

impl Lane {
    fn controller(self) -> Option<u8> {
        match self {
            Self::Expression => Some(11),
            Self::Sustain => Some(64),
            _ => None,
        }
    }
}

const LANE_HEIGHT: i32 = 64;

pub fn note_height(frame: &Frame, model: &Model) -> i32 {
    frame.h()
        - RULER
        - if model.edit_lane == Lane::Hidden {
            0
        } else {
            LANE_HEIGHT
        }
}

fn lane_point(frame: &Frame, model: &Model, x: i32, y: i32) -> (Tick, u8) {
    let left = frame.x() + roll_gutter(model);
    let width = frame.w() - roll_gutter(model);
    let ppq = u64::from(model.song.ppq);
    let tick = model.view_beat * ppq
        + x.saturating_sub(left).clamp(0, width - 1) as u64 * (BEATS * ppq) / width as u64;
    let value =
        ((frame.y() + frame.h() - 1 - y).clamp(0, LANE_HEIGHT - 1) * 127 / (LANE_HEIGHT - 1)) as u8;
    (tick / model.step * model.step, value)
}

fn draw_lane(frame: &Frame, model: &Model) {
    if model.edit_lane == Lane::Hidden {
        return;
    }
    let left = frame.x() + roll_gutter(model);
    let bottom = frame.y() + frame.h();
    let top = bottom - LANE_HEIGHT;
    draw::draw_rect_fill(frame.x(), top, frame.w(), LANE_HEIGHT, BG);
    draw::set_draw_color(SUBTLE);
    let caption = match model.edit_lane {
        Lane::Hidden => "",
        Lane::Velocity => "Velocity",
        Lane::Expression => "CC11 Expr.",
        Lane::Sustain => "CC64 Pedal",
    };
    draw::draw_text2(
        caption,
        frame.x() + 3,
        top + 3,
        roll_gutter(model) - 4,
        16,
        Align::Inside | Align::Left | Align::Clip,
    );
    let ppq = u64::from(model.song.ppq);
    let first = model.view_beat * ppq;
    let last = first + BEATS * ppq;
    let points: Vec<_> = if let Some(cc) = model.edit_lane.controller() {
        model.song.tracks[model.track]
            .controller_points(cc)
            .filter(|p| (first..last).contains(&p.0))
            .collect()
    } else {
        model
            .notes
            .iter()
            .filter(|n| (first..last).contains(&n.start))
            .map(|n| (n.start, n.velocity))
            .collect()
    };
    draw::push_clip(left, top, frame.w() - roll_gutter(model), LANE_HEIGHT);
    for (tick, value) in points {
        let h = 1 + i32::from(value) * (LANE_HEIGHT - 2) / 127;
        draw::draw_rect_fill(tick_x(frame, model, tick), bottom - h, 3, h, ACCENT);
    }
    draw::pop_clip();
}

pub fn paint_lane(
    model: &mut Model,
    lane: Lane,
    mut from: (Tick, u8),
    mut to: (Tick, u8),
) -> song::Result<()> {
    if from.0 > to.0 {
        std::mem::swap(&mut from, &mut to);
    }
    let len = to.0 - from.0;
    let value_at = |tick| {
        let alpha = if len == 0 {
            1.0
        } else {
            (tick - from.0) as f64 / len as f64
        };
        (f64::from(from.1) + f64::from(i16::from(to.1) - i16::from(from.1)) * alpha).round() as u8
    };
    if lane == Lane::Velocity {
        let original: Vec<_> = model
            .notes
            .iter()
            .copied()
            .filter(|n| (from.0..=to.0).contains(&(n.start / model.step * model.step)))
            .collect();
        if !original.is_empty() {
            let changed = original
                .iter()
                .map(|n| Note {
                    velocity: value_at(n.start / model.step * model.step).max(1),
                    ..*n
                })
                .collect();
            update(model, original, changed)?;
        }
    } else if let Some(cc) = lane.controller() {
        let points: Vec<_> = (from.0..=to.0)
            .step_by(model.step as usize)
            .map(|tick| {
                let value = value_at(tick);
                (
                    tick,
                    if cc == 64 {
                        if value >= 64 {
                            127
                        } else {
                            0
                        }
                    } else {
                        value
                    },
                )
            })
            .collect();
        model.checkpoint();
        model.selected.clear();
        model.song.tracks[model.track].paint_controller(cc, &points);
    }
    Ok(())
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
            Tool::Tone | Tool::Chord | Tool::Inversion | Tool::ArpUp | Tool::ArpDown | Tool::Motif
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
            draw_lane(f, &m);
            draw_preview(f, &m, &m.pattern);
            if let Some(drag) = *preview.borrow() {
                if drag.gesture.track == m.track && drag.start != drag.end {
                    draw_drag(f, &m, drag);
                }
            }
            track_view::draw_playhead(f, &m, roll_gutter(&m));
        }
    });
    let data = Rc::clone(model);
    let mut lane_start = None;
    frame.handle(move |f, event| {
        let Ok(m) = data.try_borrow() else {
            return false;
        };
        if drag.borrow().is_none() && track_view::seek_ruler(f, &m, event, roll_gutter(&m), tx) {
            return true;
        }
        let (x, y) = (app::event_x(), app::event_y());
        if let Some((track, lane, point)) = lane_start {
            match event {
                Event::Released => {
                    lane_start = None;
                    tx.send(Action::PaintLane(
                        track,
                        lane,
                        point,
                        lane_point(f, &m, x, y),
                    ));
                    return true;
                }
                Event::Drag => return true,
                Event::Unfocus => lane_start = None,
                _ => {}
            }
        }
        if event == Event::Push
            && app::event_mouse_button() == MouseButton::Left
            && m.edit_lane != Lane::Hidden
            && y >= f.y() + f.h() - LANE_HEIGHT
            && (f.x() + roll_gutter(&m)..f.x() + f.w()).contains(&x)
        {
            let _ = f.take_focus();
            lane_start = Some((m.track, m.edit_lane, lane_point(f, &m, x, y)));
            return true;
        }
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
                tx.send(Action::CancelPattern);
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
        end.1
            .clamp(f.y() + RULER, f.y() + RULER + note_height(f, model) - 1),
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
                let height = note_height(f, m);
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

fn draw_preview(f: &Frame, model: &Model, notes: &[Note]) {
    draw::set_draw_color(Color::from_rgb(177, 151, 255));
    let top = f.y() + RULER;
    let height = note_height(f, model);
    for note in notes {
        let row = i32::from(model.high_key) - i32::from(note.key);
        let start = tick_x(f, model, note.start);
        let end = tick_x(f, model, note.end);
        if (0..ROWS).contains(&row) && end > start {
            let y = top + row * height / ROWS;
            let h = top + (row + 1) * height / ROWS - y;
            draw::draw_rect(start, y + 1, end - start, (h - 2).max(1));
        }
    }
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
            model.pattern.clear();
            model.pattern = if tool == Tool::Motif {
                motif_notes(model, origin.tick, end.tick)?
            } else {
                chord_notes(model, tool, origin, length, velocity)?
            };
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
    let mut keys = chord.voicing(point.key, tool == Tool::Inversion);
    if model.pattern_spread && keys.len() > 1 {
        let last = keys.len() - 1;
        if keys[last] <= 115 {
            keys[last] += 12;
        }
    }
    let interval = if model.pattern_triplet {
        (u64::from(model.song.ppq) / 3).max(1)
    } else {
        model.step
    };
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
                let at = start.saturating_add(interval.saturating_mul(i as Tick));
                if at >= limit {
                    break;
                }
                notes.push(new_note(
                    at,
                    at.saturating_add(length.min(interval)).min(limit),
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

fn motif_notes(model: &Model, from: Tick, to: Tick) -> song::Result<Vec<Note>> {
    let notes = &model.note_clipboard;
    let Some(first) = notes.iter().map(|n| n.start).min() else {
        return Err("Select a motif and copy it with Ctrl+C first.".into());
    };
    let channel = model.song.tracks[model.track].channel;
    if channel == 9 {
        return Err("Choose a melodic track for the motif.".into());
    }
    let duration = notes
        .iter()
        .map(|n| n.end - first)
        .max()
        .unwrap_or(1)
        .max(1);
    let start = from.min(to) / model.step * model.step;
    let end = if from == to {
        start.saturating_add(duration)
    } else {
        to.max(from).saturating_add(model.step - 1) / model.step * model.step
    };
    if (end - start)
        .div_ceil(duration)
        .saturating_mul(notes.len() as u64)
        > 4096
    {
        return Err("Select a shorter motif range or copy fewer notes.".into());
    }
    let mut fitted = Vec::new();
    for base in (start..end).step_by(duration as usize) {
        for note in notes {
            let tick = base.saturating_add(note.start - first);
            if tick >= end {
                continue;
            }
            let i = model.chords.partition_point(|&(at, _)| at <= tick);
            let Some((_, Some(chord))) = i.checked_sub(1).and_then(|i| model.chords.get(i)) else {
                continue;
            };
            let boundary = model.chords.get(i).map_or(end, |&(at, _)| at.min(end));
            let finish = tick
                .saturating_add((note.end - note.start).max(1))
                .min(boundary);
            let key = (0_u8..=127)
                .filter(|&key| chord.contains(key))
                .filter(|key| {
                    fitted
                        .iter()
                        .all(|n: &Note| n.key != *key || n.end <= tick || n.start >= finish)
                })
                .min_by_key(|key| key.abs_diff(note.key));
            if let Some(key) = key {
                fitted.push(new_note(tick, finish, key, note.velocity, channel));
            }
        }
    }
    if fitted.is_empty() {
        return Err("Select a range with detected harmony.".into());
    }
    Ok(fitted)
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
        model.cache_music();
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
    #[test]
    fn motif_repeats_with_the_progression_and_cache_tracks_mute_and_undo() {
        let mut model = Model::new();
        let track = &mut model.song.tracks[0];
        for (tick, keys) in [(0, [60, 64, 67]), (480, [62, 65, 69])] {
            for key in keys {
                track.add_note(tick, tick + 480, key, 96);
            }
        }
        model.cache_music();
        model.song.add_track();
        model.track = 1;
        model.note_clipboard = vec![new_note(200, 680, 64, 91, 0)];
        let notes = motif_notes(&model, 0, 960).unwrap();
        assert_eq!(
            notes
                .iter()
                .map(|n| (n.start, n.end, n.key, n.velocity))
                .collect::<Vec<_>>(),
            [(0, 480, 64, 91), (480, 960, 65, 91)]
        );
        model.checkpoint();
        model.song.tracks[0].mute = true;
        model.cache_music();
        assert!(model.chords.is_empty());
        model.history(false);
        model.cache_music();
        assert_eq!(motif_notes(&model, 0, 960).unwrap(), notes);
    }

    #[test]
    fn automation_stroke_replaces_existing_ticks_and_is_undoable() {
        let mut model = note_model();
        paint_lane(&mut model, Lane::Expression, (0, 0), (240, 100)).unwrap();
        assert_eq!(
            model.song.tracks[0]
                .controller_points(11)
                .collect::<Vec<_>>(),
            [(0, 0), (120, 50), (240, 100)]
        );
        paint_lane(&mut model, Lane::Expression, (120, 3), (120, 80)).unwrap();
        let points: Vec<_> = model.song.tracks[0].controller_points(11).collect();
        assert_eq!(
            points
                .iter()
                .filter(|p| p.0 == 120)
                .copied()
                .collect::<Vec<_>>(),
            [(120, 80)]
        );
        model.history(false);
        assert_eq!(
            model.song.tracks[0]
                .controller_points(11)
                .collect::<Vec<_>>(),
            [(0, 0), (120, 50), (240, 100)]
        );
        paint_lane(&mut model, Lane::Sustain, (0, 62), (120, 65)).unwrap();
        assert_eq!(
            model.song.tracks[0]
                .controller_points(64)
                .collect::<Vec<_>>(),
            [(0, 0), (120, 127)]
        );
    }
}
