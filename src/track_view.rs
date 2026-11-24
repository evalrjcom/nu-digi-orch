use super::*;

pub(super) const GUTTER: i32 = 142;
const LANE: i32 = 44;

fn first_lane(frame: &Frame, model: &Model) -> usize {
    let rows = ((frame.h() - RULER) / LANE).max(1) as usize;
    model.track / rows * rows
}

pub(super) fn handle(frame: &Frame, model: &Model, event: Event, tx: app::Sender<Action>) -> bool {
    match event {
        Event::MouseWheel => {
            let index = model
                .track
                .saturating_add_signed(app::event_dy_value() as isize)
                .min(model.song.tracks.len() - 1);
            tx.send(Action::Lane(index, false));
            true
        }
        Event::Push if app::event_mouse_button() == MouseButton::Left => {
            let y = app::event_y() - frame.y() - RULER;
            if y >= 0 {
                let index = first_lane(frame, model) + (y / LANE) as usize;
                if index < model.song.tracks.len() {
                    tx.send(Action::Lane(index, app::event_clicks()));
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
    draw::draw_text2(
        "Track",
        x + 7,
        y,
        GUTTER - 7,
        RULER,
        Align::Inside | Align::Left,
    );
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
        let label = format!("{:02}  {}", index + 1, track.name());
        draw::draw_text2(
            &label,
            x + 7,
            top + 2,
            GUTTER - 12,
            20,
            Align::Inside | Align::Left | Align::Clip,
        );
        draw::set_draw_color(SUBTLE);
        let state = if track.mute {
            "  Mute"
        } else if track.solo {
            "  Solo"
        } else {
            ""
        };
        draw::draw_text2(
            &format!("Ch {}{}", track.channel + 1, state),
            x + 8,
            top + 22,
            GUTTER - 12,
            18,
            Align::Inside | Align::Left,
        );
        let ink = if track.mute || (solo && !track.solo) {
            SUBTLE
        } else {
            ACCENT
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
            y,
            55,
            RULER,
            Align::Inside | Align::Left,
        );
    }
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
