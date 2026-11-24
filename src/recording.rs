use crate::song::Result;
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use std::{sync::mpsc, time::Instant};

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

pub struct Recording {
    started: Instant,
    connection: MidiInputConnection<()>,
    received: mpsc::Receiver<(u64, [u8; 3])>,
}

impl Recording {
    pub fn start(port: &MidiInputPort) -> Result<Self> {
        let mut midi = MidiInput::new("Nu Digi Orch")?;
        midi.ignore(Ignore::All);
        let (send, received) = mpsc::sync_channel(8192);
        let started = Instant::now();
        let connection = midi
            .connect(
                port,
                "Composer input",
                move |_, bytes, _| {
                    if (2..=3).contains(&bytes.len()) && (0x80..0xf0).contains(&bytes[0]) {
                        let mut message = [0; 3];
                        message[..bytes.len()].copy_from_slice(bytes);
                        let _ = send.try_send((started.elapsed().as_micros() as u64, message));
                    }
                },
                (),
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            started,
            connection,
            received,
        })
    }

    pub fn elapsed(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }

    pub fn drain(&self, into: &mut Vec<(u64, [u8; 3])>) {
        into.extend(self.received.try_iter());
    }

    pub fn finish(self, mut events: Vec<(u64, [u8; 3])>) -> (u64, Vec<(u64, [u8; 3])>) {
        let elapsed = self.elapsed();
        self.connection.close();
        events.extend(self.received.try_iter());
        (elapsed, events)
    }
}
