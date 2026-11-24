use super::*;

const STRIP: i32 = 132;
const CONTROL: Color = Color::from_rgb(54, 68, 86);

fn page(f: &Frame, m: &Model) -> (usize, usize) {
    let count = (f.w() / STRIP - 1).max(1) as usize;
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

fn eq_meter(x: i32, y: i32, bottom: i32, bus: &crate::bus::Bus, playing: bool) {
    let [bass, treble, _, _] = bus.get();
    for (row, (name, v)) in [("Bass", bass), ("Treble", treble)].into_iter().enumerate() {
        let top = y + 105 + row as i32 * 28;
        label(
            x + 2,
            top,
            112,
            18,
            &format!("{name} {:+} dB", v as i16 - 12),
            SUBTLE,
        );
        draw::draw_rect_fill(x + 12, top + 20, 92, 2, CONTROL);
        draw::draw_rect_fill(x + 10 + i32::from(v) * 92 / 24, top + 17, 5, 8, ACCENT);
    }
    let top = y + 190;
    let db = if playing {
        20. * bus.rms().max(0.001).log10()
    } else {
        -60.
    };
    let level = ((db + 60.) / 60.).clamp(0., 1.);
    draw::draw_rect_fill(x + 89, top, 9, bottom - top, CONTROL);
    let pixels = ((bottom - top) as f32 * level) as i32;
    draw::draw_rect_fill(
        x + 89,
        bottom - pixels,
        9,
        pixels,
        if db >= 0. { Color::Red } else { ACCENT },
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
            y + 27,
            w,
            17,
            &format!("Ch {}   Pgm {}", t.channel + 1, t.setting(None)),
            SUBTLE,
        );
        draw::set_font(Font::Helvetica, 11);
        label(x, y + 43, w, 16, gm::resolved(t, m.font.as_deref()), TEXT);
        draw::set_font(Font::Helvetica, 13);
        let pan = i32::from(t.setting(Some(10)));
        label(x, y + 60, w, 21, &format!("Pan {pan}"), TEXT);
        draw::draw_rect_fill(x + 12, y + 87, 92, 3, CONTROL);
        draw::draw_rect_fill(x + 10 + pan * 92 / 127, y + 80, 5, 16, ACCENT);
        let volume = i32::from(t.setting(Some(7)));
        eq_meter(
            x,
            y,
            bottom,
            &t.bus,
            m.playback.as_ref().is_some_and(|p| p.is_software()),
        );
        label(x, y + 163, w, 24, &format!("Volume {volume}"), TEXT);
        let top = y + 190;
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
    let x = f.x() + f.w() - STRIP;
    let y = f.y();
    draw::draw_rect_fill(x, y, STRIP - 4, f.h(), PANEL);
    label(x, y + 9, STRIP - 4, 26, "MASTER", ACCENT);
    label(x, y + 34, STRIP - 4, 20, "SoundFont", SUBTLE);
    eq_meter(
        x,
        y,
        bottom,
        &m.effects.1,
        m.playback.as_ref().is_some_and(|p| p.is_software()),
    );
    let gain = i32::from(m.effects.1.get()[2]);
    label(x, y + 163, STRIP - 4, 24, &format!("Gain {gain}%"), TEXT);
    let top = y + 190;
    let knob = bottom - gain * (bottom - top) / 127;
    draw::draw_rect_fill(x + 55, top, 6, bottom - top, CONTROL);
    draw::draw_rect_fill(x + 39, knob - 4, 38, 9, ACCENT);
    draw::pop_clip();
}

// Keep the original strip and control captured until the mouse is released.
pub(super) fn handle(
    f: &Frame,
    m: &Model,
    e: Event,
    tx: app::Sender<Action>,
    drag: &mut Option<(usize, usize, i32)>,
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
            let master = x >= f.w() - STRIP;
            let i = if master { usize::MAX } else { first + strip };
            if !master && (strip >= count || i >= m.song.tracks.len()) {
                return false;
            }
            let local = x % STRIP;
            if !master
                && (bottom + 19..bottom + 47).contains(&y)
                && ((5..56).contains(&local) || (60..111).contains(&local))
            {
                tx.send(Action::Cell(i, if local < 56 { 7 } else { 8 }));
                return true;
            }
            let col = if (105..133).contains(&y) {
                8
            } else if (133..161).contains(&y) {
                9
            } else if !master && (73..103).contains(&y) {
                6
            } else if (183..=bottom + 6).contains(&y) {
                5
            } else {
                if !master {
                    tx.send(Action::Lane(i, false));
                }
                return true;
            };
            // Preserve the strip origin if scrolling or resizing occurs during the gesture.
            let origin = if master {
                f.w() - STRIP
            } else {
                strip as i32 * STRIP
            };
            *drag = Some((i, col, origin));
        }
        Event::Drag if drag.is_some() => {}
        _ => return false,
    }
    if let Some((i, col, origin)) = *drag {
        let local = x - origin;
        if col == 8 || col == 9 {
            tx.send(Action::BusValue(
                i,
                col,
                (24 * (local - 12).clamp(0, 92) / 92 - 12) as i16,
            ));
            return true;
        }
        let value = if col == 5 {
            127 * (bottom - y).clamp(0, bottom - 190) / (bottom - 190)
        } else {
            127 * (local - 12).clamp(0, 92) / 92
        };
        if i == usize::MAX {
            tx.send(Action::BusValue(i, col, value as i16));
        } else {
            tx.send(Action::MixerValue(i, col, value as u8));
        }
    }
    true
}
