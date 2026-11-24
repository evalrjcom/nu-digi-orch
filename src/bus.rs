use std::sync::atomic::{AtomicU32, Ordering};
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
    pub fn rms(&self) -> f32 {
        f32::from_bits(self.meter.load(Ordering::Relaxed))
    }
}
pub struct Filter {
    low: [f32; 2],
    mid: [f32; 2],
    a: f32,
    b: f32,
    rms: f32,
    rate: f32,
}
impl Filter {
    pub fn new(rate: u32) -> Self {
        let coeff = |hz: f32| 1. - (-std::f32::consts::TAU * hz / rate as f32).exp();
        Self {
            low: [0.; 2],
            mid: [0.; 2],
            a: coeff(200.),
            b: coeff(4000.),
            rms: 0.,
            rate: rate as f32,
        }
    }
    pub fn process(&mut self, l: &mut [f32], r: &mut [f32], bus: &Bus) {
        let [bass, treble, gain, _] = bus.get();
        let shelf = |v: u8| 10f32.powf((v as f32 - 12.) / 20.) - 1.;
        let (lo, hi) = (shelf(bass), shelf(treble));
        let mut energy = 0.;
        let n = l.len();
        if n == 0 {
            return;
        }
        for (l, r) in l.iter_mut().zip(r) {
            for (i, x) in [l, r].into_iter().enumerate() {
                self.low[i] += self.a * (*x - self.low[i]);
                self.mid[i] += self.b * (*x - self.mid[i]);
                *x = (*x + lo * self.low[i] + hi * (*x - self.mid[i])) * gain as f32 / 100.;
                energy += *x * *x;
            }
        }
        let smoothing = 1. - (-(n as f32) / (0.3 * self.rate)).exp();
        self.rms += smoothing * (energy / (2 * n) as f32 - self.rms);
        bus.meter
            .store(self.rms.max(0.).sqrt().to_bits(), Ordering::Relaxed);
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
        f.process(&mut l, &mut r, &bus);
        assert_eq!(l, [0.25; 256]);
        assert!(bus.rms() > 0.);
        bus.set(3, 4, 0);
        f.process(&mut l, &mut r, &bus);
        assert_eq!(r, [0.; 256]);
    }
}
