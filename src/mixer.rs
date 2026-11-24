use super::*;

const STRIP: i32 = 120;
const CONTROL: Color = Color::from_rgb(54, 68, 86);

fn page(f: &Frame, m: &Model) -> (usize, usize) {
    let count = (f.w() / STRIP).max(1) as usize;
    (m.track / count * count, count)
}

fn label(x: i32, y: i32, w: i32, h: i32, text: &str, color: Color) {
    draw::set_draw_color(color);
    draw::draw_text2(
        text,
        x,
        y,
        w,
        h,
        Align::Inside | Align::Center | Align::Clip,
    );
}

pub(super) fn paint(f: &Frame, m: &Model) {
    draw::push_clip(f.x(), f.y(), f.w(), f.h());
    draw::draw_rect_fill(f.x(), f.y(), f.w(), f.h(), BG);
    draw::set_font(Font::Helvetica, 13);
    let (first, count) = page(f, m);
    let bottom = f.y() + f.h() - 60;
    for (n, t) in m.song.tracks.iter().enumerate().skip(first).take(count) {
        let x = f.x() + ((n - first) as i32) * STRIP;
        let y = f.y();
        let w = STRIP - 4;
        draw::draw_rect_fill(x, y, w, f.h(), PANEL);
        if n == m.track {
            draw::draw_rect_fill(x, y, w, 3, ACCENT);
        }
        label(
            x,
            y + 7,
            w,
            23,
            &format!("{:02}  {}", n + 1, t.name()),
            TEXT,
        );
        label(
            x,
            y + 32,
            w,
            21,
            &format!("Ch {}   Pgm {}", t.channel + 1, t.setting(None)),
            SUBTLE,
        );
        let pan = i32::from(t.setting(Some(10)));
        label(x, y + 57, w, 21, &format!("Pan {pan}"), TEXT);
        draw::draw_rect_fill(x + 12, y + 87, 92, 3, CONTROL);
        draw::draw_rect_fill(x + 10 + pan * 92 / 127, y + 80, 5, 16, ACCENT);
        let volume = i32::from(t.setting(Some(7)));
        label(x, y + 108, w, 24, &format!("Volume {volume}"), TEXT);
        let top = y + 147;
        let knob = bottom - volume * (bottom - top) / 127;
        draw::draw_rect_fill(x + 55, top, 6, bottom - top + 1, CONTROL);
        draw::draw_rect_fill(x + 55, knob, 6, bottom - knob + 1, ACCENT);
        draw::draw_rect_fill(x + 39, knob - 4, 38, 9, TEXT);
        for (offset, name, on) in [(5, "Mute", t.mute), (60, "Solo", t.solo)] {
            draw::draw_rect_fill(
                x + offset,
                bottom + 19,
                51,
                28,
                if on { ACCENT } else { CONTROL },
            );
            label(
                x + offset,
                bottom + 19,
                51,
                28,
                name,
                if on { BG } else { TEXT },
            );
        }
    }
    draw::pop_clip();
}

// Keep the original strip and control captured until the mouse is released.
pub(super) fn handle(
    f: &Frame,
    m: &Model,
    e: Event,
    tx: app::Sender<Action>,
    drag: &mut Option<(usize, usize)>,
) -> bool {
    let (first, count) = page(f, m);
    let x = app::event_x() - f.x();
    let y = app::event_y() - f.y();
    let bottom = f.h() - 60;
    match e {
        Event::MouseWheel => {
            let i = m
                .track
                .saturating_add_signed(app::event_dy_value() as isize)
                .min(m.song.tracks.len() - 1);
            tx.send(Action::Lane(i, false));
            return true;
        }
        Event::Released => {
            return drag.take().is_some();
        }
        Event::Push if app::event_mouse_button() == MouseButton::Left && x >= 0 => {
            let strip = x as usize / STRIP as usize;
            let i = first + strip;
            if strip >= count || i >= m.song.tracks.len() {
                return false;
            }
            let local = x % STRIP;
            if (bottom + 19..bottom + 47).contains(&y) && (5..111).contains(&local) {
                tx.send(Action::Cell(i, if local < 59 { 7 } else { 8 }));
                return true;
            }
            let col = if (73..103).contains(&y) {
                6
            } else if (139..=bottom + 6).contains(&y) {
                5
            } else {
                tx.send(Action::Lane(i, false));
                return true;
            };
            *drag = Some((i, col));
        }
        Event::Drag if drag.is_some() => {}
        _ => return false,
    }
    if let Some((i, col)) = *drag {
        let value = if col == 5 {
            127 * (bottom - y).clamp(0, bottom - 147) / (bottom - 147)
        } else {
            let local = x - (i - first) as i32 * STRIP;
            127 * (local - 12).clamp(0, 92) / 92
        };
        tx.send(Action::MixerValue(i, col, value as u8));
    }
    true
}
