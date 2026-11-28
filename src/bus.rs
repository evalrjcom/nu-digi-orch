use std::sync::atomic::{AtomicU32, Ordering};
#[derive(Clone, Copy, Default)]
pub struct Meter {
    pub peak: f32,
    pub clipping: bool,
}

pub struct Bus {
    settings: AtomicU32,
    meter: AtomicU32,
    fx: AtomicU32,
}
impl Default for Bus {
    fn default() -> Self {
        Self {
            settings: AtomicU32::new(u32::from_le_bytes([12, 12, 100, 0])),
            meter: AtomicU32::new(0),
            fx: AtomicU32::new(0),
        }
    }
}
impl Bus {
    pub fn get(&self) -> [u8; 4] {
        self.settings.load(Ordering::Relaxed).to_le_bytes()
    }
    pub fn set(&self, bass: i8, treble: i8, gain: u8) {
        self.settings.store(
            u32::from_le_bytes([
                (bass.clamp(-12, 12) + 12) as u8,
                (treble.clamp(-12, 12) + 12) as u8,
                gain.min(127),
                0,
            ]),
            Ordering::Relaxed,
        );
    }
    pub fn snapshot(&self) -> Self {
        Self {
            settings: AtomicU32::new(self.settings.load(Ordering::Relaxed)),
            fx: AtomicU32::new(self.fx.load(Ordering::Relaxed)),
            ..Self::default()
        }
    }
    pub fn effects(&self) -> [u8; 3] {
        let [reverb, chorus, delay, _] = self.fx.load(Ordering::Relaxed).to_le_bytes();
        [reverb, chorus, delay]
    }
    pub fn set_effect(&self, index: usize, amount: u8) {
        let mut bytes = self.effects();
        bytes[index] = amount.min(100);
        self.fx.store(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]),
            Ordering::Relaxed,
        );
    }
    pub fn meter(&self) -> Meter {
        let value = self.meter.load(Ordering::Relaxed);
        Meter {
            peak: f32::from_bits(value & 0x7fff_ffff),
            clipping: value >> 31 != 0,
        }
    }
    pub fn set_meter(&self, meter: Meter) {
        // A nonnegative peak leaves its sign bit available for the clip indicator.
        let value = meter.peak.to_bits() | (u32::from(meter.clipping) << 31);
        self.meter.store(value, Ordering::Relaxed);
    }
}
pub struct Filter {
    low: [f32; 2],
    mid: [f32; 2],
    a: f32,
    b: f32,
    peak: f32,
    clip_frames: usize,
    rate: u32,
}
impl Filter {
    pub fn new(rate: u32) -> Self {
        let coeff = |hz: f32| 1. - (-std::f32::consts::TAU * hz / rate as f32).exp();
        Self {
            low: [0.; 2],
            mid: [0.; 2],
            a: coeff(200.),
            b: coeff(4000.),
            peak: 0.,
            clip_frames: 0,
            rate,
        }
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], bus: &Bus) -> Meter {
        let [bass, treble, gain, _] = bus.get();
        let shelf = |v: u8| 10f32.powf((v as f32 - 12.) / 20.) - 1.;
        let (lo, hi) = (shelf(bass), shelf(treble));
        let mut peak = 0.0_f32;
        let n = l.len();
        for (l, r) in l.iter_mut().zip(r) {
            for (i, x) in [l, r].into_iter().enumerate() {
                self.low[i] += self.a * (*x - self.low[i]);
                self.mid[i] += self.b * (*x - self.mid[i]);
                *x = (*x + lo * self.low[i] + hi * (*x - self.mid[i])) * gain as f32 / 100.;
                peak = peak.max(x.abs());
            }
        }
        // Fast attack and a gradual release keep a one-sample transient visible.
        let decay = (-(n as f32) / (0.3 * self.rate as f32)).exp();
        self.peak = (self.peak * decay).max(peak);
        self.clip_frames = if peak > 1. {
            self.rate as usize
        } else {
            self.clip_frames.saturating_sub(n)
        };
        Meter {
            peak: self.peak,
            clipping: self.clip_frames > 0,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silent_master_and_flat_unity() {
        let bus = Bus::default();
        let mut f = Filter::new(44100);
        let mut l = [0.25; 256];
        let mut r = l;
        let meter = f.process(&mut l, &mut r, &bus);
        assert_eq!(l, [0.25; 256]);
        assert_eq!(meter.peak, 0.25);
        assert!(!meter.clipping);
        bus.set(3, 4, 0);
        f.process(&mut l, &mut r, &bus);
        assert_eq!(r, [0.; 256]);
    }

    #[test]
    fn clip_holds_for_one_second_after_a_single_overload() {
        let bus = Bus::default();
        let mut filter = Filter::new(48_000);
        let meter = filter.process(&mut [0.], &mut [-1.], &bus);
        assert_eq!(meter.peak, 1.);
        assert!(!meter.clipping);

        bus.set(0, 0, 127);
        bus.set_meter(filter.process(&mut [0.], &mut [-1.], &bus));
        assert_eq!(bus.meter().peak, 1.27);
        assert!(bus.meter().clipping);
        assert!(
            filter
                .process(&mut [0.; 256], &mut [0.; 256], &bus)
                .clipping
        );

        for _ in 1..187 {
            filter.process(&mut [0.; 256], &mut [0.; 256], &bus);
        }
        assert!(
            filter
                .process(&mut [0.; 127], &mut [0.; 127], &bus)
                .clipping
        );
        let released = filter.process(&mut [0.], &mut [0.], &bus);
        assert!(!released.clipping);
        assert!(released.peak < 0.05);
    }
}
