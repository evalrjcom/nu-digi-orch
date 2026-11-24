use crate::song::Result;
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Instant,
};

pub struct InputPort {
    pub port: MidiInputPort,
    pub name: String,
}

pub fn inputs() -> Result<Vec<InputPort>> {
    let midi = MidiInput::new("Nu Digi Orch")?;
    Ok(midi
        .ports()
        .into_iter()
        .filter_map(|port| {
            let name = midi.port_name(&port).ok()?;
            Some(InputPort { port, name })
        })
        .collect())
}

// MIDI program change and channel pressure carry one data byte; other channel messages carry two.
fn channel_message(bytes: &[u8]) -> Option<[u8; 3]> {
    let (&status, data) = bytes.split_first()?;
    if !(0x80..0xf0).contains(&status) || data.iter().any(|b| *b >= 128) {
        return None;
    }
    let len = if matches!(status >> 4, 0xc | 0xd) {
        1
    } else {
        2
    };
    if data.len() != len {
        return None;
    }
    Some([status, data[0], data.get(1).copied().unwrap_or(0)])
}

pub struct Recording {
    dropped: Arc<AtomicUsize>,
    started: Instant,
    connection: Option<MidiInputConnection<()>>,
    received: mpsc::Receiver<(u64, [u8; 3])>,
}

pub struct Capture {
    send: mpsc::SyncSender<(u64, [u8; 3])>,
    started: Instant,
    dropped: Arc<AtomicUsize>,
}
impl Capture {
    pub fn send(&self, bytes: [u8; 3]) {
        if let Err(mpsc::TrySendError::Full(_)) = self
            .send
            .try_send((self.started.elapsed().as_micros() as u64, bytes))
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Recording {
    pub fn virtual_piano() -> (Self, Capture) {
        let (send, received) = mpsc::sync_channel(8192);
        let started = Instant::now();
        let dropped = Arc::new(AtomicUsize::new(0));
        let capture = Capture {
            send,
            started,
            dropped: dropped.clone(),
        };
        (
            Self {
                dropped,
                started,
                connection: None,
                received,
            },
            capture,
        )
    }

    pub fn start(port: &MidiInputPort) -> Result<Self> {
        let mut midi = MidiInput::new("Nu Digi Orch")?;
        midi.ignore(Ignore::All);
        let (send, received) = mpsc::sync_channel(8192);
        let dropped = Arc::new(AtomicUsize::new(0));
        let lost = dropped.clone();
        let started = Instant::now();
        let connection = midi
            .connect(
                port,
                "Composer input",
                move |_, bytes, _| {
                    if let Some(message) = channel_message(bytes) {
                        // Never block the MIDI input callback; report queue overruns when the take ends.
                        if let Err(mpsc::TrySendError::Full(_)) =
                            send.try_send((started.elapsed().as_micros() as u64, message))
                        {
                            lost.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                },
                (),
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            dropped,
            started,
            connection: Some(connection),
            received,
        })
    }

    pub fn elapsed(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    pub fn drain(&self, into: &mut Vec<(u64, [u8; 3])>) {
        into.extend(self.received.try_iter());
    }

    pub fn finish(self, mut events: Vec<(u64, [u8; 3])>) -> (u64, Vec<(u64, [u8; 3])>, usize) {
        let elapsed = self.elapsed();
        if let Some(connection) = self.connection {
            connection.close();
        }
        events.extend(self.received.try_iter());
        (elapsed, events, self.dropped.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_message_lengths_and_data_bytes() {
        assert_eq!(channel_message(&[0xc2, 12]), Some([0xc2, 12, 0]));
        assert_eq!(channel_message(&[0x92, 60, 90]), Some([0x92, 60, 90]));
        for bytes in [&[0x90, 60][..], &[0xc0, 4, 5], &[0x90, 128, 9], &[]] {
            assert!(channel_message(bytes).is_none());
        }
    }
}
