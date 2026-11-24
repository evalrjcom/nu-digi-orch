use super::*;

pub(super) const GUTTER: i32 = 561;
const LANE: i32 = 29;

const OFFSETS: [i32; 11] = [0, 29, 149, 181, 228, 271, 312, 350, 369, 388, GUTTER];
pub(super) const HEADERS: [&str; 10] = [
    "#",
    "Name",
    "Ch",
    "Prog",
    "Bank",
    "Vol",
    "Pan",
    "M",
    "S",
    "Instrument",
];
pub(super) fn values(i: usize, t: &song::Track) -> [String; 10] {
    [
        (i + 1).to_string(),
        t.name(),
        (t.channel + 1).to_string(),
        t.setting(None).to_string(),
        t.setting(Some(0)).to_string(),
        t.setting(Some(7)).to_string(),
        t.setting(Some(10)).to_string(),
        if t.mute { "M" } else { "·" }.into(),
        if t.solo { "S" } else { "·" }.into(),
        gm::instrument(t).into(),
    ]
}
fn cells(x: i32, y: i32, labels: impl IntoIterator<Item = impl AsRef<str>>) {
    for (i, label) in labels.into_iter().enumerate() {
        draw::draw_text2(
            label.as_ref(),
            x + OFFSETS[i] + 2,
            y,
            OFFSETS[i + 1] - OFFSETS[i] - 2,
            LANE,
            Align::Inside | Align::Left | Align::Clip,
        );
    }
}

pub(super) fn visible_rows(frame: &Frame) -> usize {
    ((frame.h() - RULER) / LANE).max(1) as usize
}
fn first_lane(frame: &Frame, model: &Model) -> usize {
    model
        .track_scroll
        .min(model.song.tracks.len().saturating_sub(visible_rows(frame)))
}

pub(super) fn seek_ruler(
    f: &Frame,
    m: &Model,
    e: Event,
    gutter: i32,
    tx: app::Sender<Action>,
) -> bool {
    let x = app::event_x() - f.x() - gutter;
    if e == Event::Push
        && app::event_mouse_button() == MouseButton::Left
        && (0..RULER).contains(&(app::event_y() - f.y()))
        && (0..f.w() - gutter).contains(&x)
    {
        let beat = m.view_beat + x as u64 * BEATS / (f.w() - gutter) as u64;
        tx.send(Action::Seek(beat * u64::from(m.song.ppq)));
        return true;
    }
    false
}
pub(super) fn handle(frame: &Frame, model: &Model, event: Event, tx: app::Sender<Action>) -> bool {
    if seek_ruler(frame, model, event, GUTTER, tx) {
        return true;
    }
    match event {
        Event::Focus | Event::Unfocus => true,
        Event::KeyDown
            if (2..=6).contains(&model.column)
                && matches!(app::event_key(), Key::Up | Key::Down) =>
        {
            tx.send(Action::Step(
                model.track,
                model.column,
                if app::event_key() == Key::Up { 1 } else { -1 },
            ));
            true
        }
        Event::MouseWheel => {
            let first = first_lane(frame, model)
                .saturating_add_signed(app::event_dy_value() as isize)
                .min(model.song.tracks.len().saturating_sub(visible_rows(frame)));
            tx.send(Action::ScrollTracks(first));
            true
        }
        Event::Push if app::event_mouse_button() == MouseButton::Left => {
            let _ = frame.clone().take_focus();
            let y = app::event_y() - frame.y() - RULER;
            if y >= 0 {
                let index = first_lane(frame, model) + (y / LANE) as usize;
                if index < model.song.tracks.len() {
                    let x = app::event_x() - frame.x();
                    let col = OFFSETS
                        .partition_point(|&offset| offset <= x)
                        .saturating_sub(1);
                    if (2..=6).contains(&col) && x >= OFFSETS[col + 1] - 12 {
                        tx.send(Action::Step(
                            index,
                            col,
                            if y % LANE < LANE / 2 { 1 } else { -1 },
                        ));
                    } else if col == 3
                        || (7..=8).contains(&col)
                        || ((1..=6).contains(&col) && app::event_clicks())
                    {
                        tx.send(Action::Cell(index, col));
                    } else {
                        if (2..=6).contains(&col) {
                            tx.send(Action::FocusCell(index, col));
                        } else {
                            tx.send(Action::Lane(index, app::event_clicks() && x >= GUTTER));
                        }
                    }
                }
            }
            true
        }
        _ => false,
    }
}

pub(super) fn draw_tracks(frame: &Frame, model: &Model) {
    let (x, y, w, h) = (frame.x(), frame.y(), frame.w(), frame.h());
    let left = x + GUTTER;
    let width = w - GUTTER;
    let ppq = u64::from(model.song.ppq);
    let start = model.view_beat * ppq;
    let span = BEATS * ppq;
    let end = start + span;
    let pos = |tick: Tick| left + ((tick - start) * width as u64 / span) as i32;
    draw::push_clip(x, y, w, h);
    draw::draw_rect_fill(x, y, w, h, PANEL);
    draw::set_font(Font::Helvetica, 12);
    draw::set_draw_color(SUBTLE);
    cells(x, y + CHORD_BAND, HEADERS);
    let first = first_lane(frame, model);
    let visible = ((h - RULER + LANE - 1) / LANE) as usize;
    let solo = model.song.tracks.iter().any(|track| track.solo);
    for (row, track) in model
        .song
        .tracks
        .iter()
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let index = first + row;
        let top = y + RULER + row as i32 * LANE;
        let fill = if index == model.track {
            Color::from_rgb(42, 56, 74)
        } else if row % 2 == 0 {
            BG
        } else {
            PANEL
        };
        draw::draw_rect_fill(x, top, w, LANE - 1, fill);
        draw::set_draw_color(TEXT);
        let colors = [
            ACCENT,
            Color::from_rgb(125, 169, 247),
            Color::from_rgb(203, 152, 230),
            Color::from_rgb(242, 177, 107),
        ];
        let color = colors[index % colors.len()];
        draw::draw_rect_fill(x, top, 3, LANE - 1, color);
        draw::set_draw_color(TEXT);
        let mut labels = values(index, track);
        labels[9] = gm::resolved(track, model.font.as_deref()).into();
        cells(x, top, labels);
        for right in &OFFSETS[3..=7] {
            let cx = x + right - 6;
            draw::draw_polygon(cx - 3, top + 9, cx + 3, top + 9, cx, top + 5);
            draw::draw_polygon(cx - 3, top + 15, cx + 3, top + 15, cx, top + 19);
        }
        let ink = if track.mute || (solo && !track.solo) {
            SUBTLE
        } else {
            color
        };
        for note in &model.lanes[index] {
            let stop = note.end.max(note.start + 1);
            if note.start >= end || stop <= start {
                continue;
            }
            let a = pos(note.start.max(start));
            let b = pos(stop.min(end));
            let height = i32::from(127 - note.key) * (LANE - 10) / 127;
            draw::draw_rect_fill(a, top + 3 + height, (b - a).max(2), 4, ink);
        }
    }
    for beat in 0..BEATS {
        let line = pos(start + beat * ppq);
        draw::set_draw_color(Color::from_rgb(63, 78, 98));
        draw::draw_line(line, y + RULER, line, y + h - 1);
        draw::set_draw_color(TEXT);
        draw::draw_text2(
            &(model.view_beat + beat + 1).to_string(),
            line + 3,
            y + CHORD_BAND,
            55,
            RULER - CHORD_BAND,
            Align::Inside | Align::Left,
        );
    }
    harmony::draw_ruler(frame, model, GUTTER);
    draw::pop_clip();
}

pub(super) fn draw_playhead(frame: &Frame, model: &Model, gutter: i32) {
    let ppq = u64::from(model.song.ppq);
    let start = model.view_beat * ppq;
    let span = BEATS * ppq;
    if !(start..start + span).contains(&model.playhead) {
        return;
    }
    let width = frame.w() - gutter;
    let x = frame.x() + gutter + ((model.playhead - start) * width as u64 / span) as i32;
    let y = frame.y();
    draw::push_clip(frame.x() + gutter, y, width, frame.h());
    let color = Color::from_rgb(255, 192, 90);
    draw::draw_rect_fill(x, y + RULER - 7, 2, frame.h() - RULER + 7, color);
    draw::set_draw_color(color);
    draw::draw_polygon(
        x - 4,
        y + RULER - 10,
        x + 5,
        y + RULER - 10,
        x,
        y + RULER - 4,
    );
    draw::pop_clip();
}
