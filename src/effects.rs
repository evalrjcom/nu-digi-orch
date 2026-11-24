use std::sync::atomic::{AtomicU32, Ordering};

// One atomic snapshot per block keeps UI changes off the audio thread's locks.
#[derive(Default)]
pub struct Levels(AtomicU32, pub crate::bus::Bus);
impl Levels {
    pub fn get(&self) -> [u8; 3] {
        let [r, c, d, _] = self.0.load(Ordering::Relaxed).to_le_bytes();
        [r, c, d]
    }
    pub fn set(&self, index: usize, value: u8) {
        let mut levels = self.get();
        levels[index] = value.min(100);
        self.0.store(
            u32::from_le_bytes([levels[0], levels[1], levels[2], 0]),
            Ordering::Relaxed,
        );
    }
    pub fn snapshot(&self) -> Self {
        Self(
            AtomicU32::new(self.0.load(Ordering::Relaxed)),
            self.1.snapshot(),
        )
    }
}

struct Line {
    samples: Vec<f32>,
    head: usize,
}
impl Line {
    fn new(size: usize) -> Self {
        Self {
            samples: vec![0.; size.max(2)],
            head: 0,
        }
    }
    fn read(&self) -> f32 {
        self.samples[self.head]
    }
    fn tap(&self, delay: f32) -> f32 {
        let n = self.samples.len();
        let whole = delay as usize;
        let a = (self.head + n - whole) % n;
        let b = (a + n - 1) % n;
        self.samples[a] + (self.samples[b] - self.samples[a]) * delay.fract()
    }
    fn push(&mut self, x: f32) {
        self.samples[self.head] = x;
        self.head = (self.head + 1) % self.samples.len();
    }
}

pub struct Rack {
    rate: f32,
    phase: f32,
    room: Vec<Line>,
    chorus: [Line; 2],
    echo: [Line; 2],
}
impl Rack {
    pub fn new(rate: u32) -> Self {
        let line = |ms: f32| Line::new((rate as f32 * ms / 1000.).round() as usize);
        Self {
            rate: rate as f32,
            phase: 0.,
            room: [29.7, 37.1, 41.1, 43.7].map(line).into(),
            chorus: [line(30.), line(30.)],
            echo: [line(300.), line(450.)],
        }
    }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], levels: [u8; 3]) {
        let [r, c, d] = levels.map(|x| f32::from(x) / 100.);
        for (l, h) in left.iter_mut().zip(right) {
            let dry = [*l, *h];
            let mono = (dry[0] + dry[1]) * 0.5;
            let taps = [
                self.room[0].read(),
                self.room[1].read(),
                self.room[2].read(),
                self.room[3].read(),
            ];
            // Orthogonal feedback mix scatters reflections across four unequal room paths.
            let sum = taps.iter().sum::<f32>() * 0.5;
            for (line, tap) in self.room.iter_mut().zip(taps) {
                line.push(mono + (sum - tap) * 0.73);
            }
            let echoes = [self.echo[0].read(), self.echo[1].read()];
            for (i, out) in [l, h].into_iter().enumerate() {
                let phase = self.phase + i as f32 * 0.25;
                let delay = self.rate * (0.016 + 0.004 * (phase * std::f32::consts::TAU).sin());
                let voice = self.chorus[i].tap(delay);
                self.chorus[i].push(dry[i]);
                self.echo[i].push(dry[i] + 0.35 * echoes[1 - i]);
                *out = dry[i]
                    + r * 0.22 * (taps[i] + taps[i + 2])
                    + c * 0.45 * voice
                    + d * 0.6 * echoes[i];
            }
            self.phase = (self.phase + 0.8 / self.rate).fract();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_effects_have_delayed_stereo_output() {
        for levels in [[0, 0, 0], [100, 0, 0], [0, 100, 0], [0, 0, 100]] {
            let mut rack = Rack::new(44100);
            let mut l = vec![0.; 44100];
            let mut r = l.clone();
            l[0] = 1.;
            r[0] = 1.;
            rack.process(&mut l, &mut r, levels);
            assert_eq!(l[0], 1.);
            assert_eq!(l[1..].iter().any(|x| x.abs() > 0.001), levels != [0, 0, 0]);
            if levels != [0, 0, 0] {
                assert_ne!(l, r);
            }
        }
    }
}
