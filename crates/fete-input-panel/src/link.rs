//! The serial port, and the thread that owns it.
//!
//! Reading a port is blocking work and the show is a render loop, so the port
//! lives on its own thread and hands frames over through a small mailbox. The
//! thread also owns reconnection: a USB cable knocked out behind a booth at
//! two in the morning should cost the panel, not the show, and should heal
//! when someone pushes the plug back in.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use serial2::SerialPort;

use crate::protocol::{Frame, parse_line};

/// How long to wait before trying a port that was not there.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Read timeout. Only bounds how long the thread blocks; frames arrive far
/// faster than this.
const READ_TIMEOUT: Duration = Duration::from_secs(1);

/// How many frames the mailbox holds before it starts dropping the oldest.
///
/// Frames carry absolute state, so under a stall the newest is the only one
/// that matters and discarding the backlog is not a loss. The queue exists at
/// all so a burst arriving between two rendered frames is not lost.
const MAILBOX_DEPTH: usize = 32;

/// What the reader thread is currently doing.
///
/// Logs are the natural place for this, but a thread that cannot open a port
/// is exactly the case where somebody is running the show without watching a
/// terminal, so the state is readable as data too.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LinkStatus {
    /// Trying to open the port.
    #[default]
    Opening,
    /// Port open, frames expected.
    Reading,
    /// The last attempt to open the port failed.
    Failed(String),
}

/// Frames waiting to be read by the show, newest last.
#[derive(Resource, Clone, Default)]
pub struct Mailbox {
    queue: Arc<Mutex<VecDeque<Frame>>>,
    status: Arc<Mutex<LinkStatus>>,
}

impl Mailbox {
    /// Take everything waiting, oldest first.
    pub fn drain(&self) -> Vec<Frame> {
        match self.queue.lock() {
            Ok(mut queue) => queue.drain(..).collect(),
            // A panicking reader thread should not take the show with it; the
            // heartbeat will notice the silence and hand the knobs back.
            Err(poisoned) => poisoned.into_inner().drain(..).collect(),
        }
    }

    /// What the reader thread is doing right now.
    pub fn status(&self) -> LinkStatus {
        match self.status.lock() {
            Ok(status) => status.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn set_status(&self, next: LinkStatus) {
        if let Ok(mut status) = self.status.lock() {
            *status = next;
        }
    }

    pub(crate) fn push(&self, frame: Frame) {
        let Ok(mut queue) = self.queue.lock() else {
            return;
        };
        if queue.len() >= MAILBOX_DEPTH {
            queue.pop_front();
        }
        queue.push_back(frame);
    }
}

/// Start reading `port`, returning the mailbox its thread fills.
///
/// Returns immediately; nothing here fails loudly, because a missing panel is
/// a normal state for this show to run in.
pub fn spawn(port: String) -> Mailbox {
    let mailbox = Mailbox::default();
    let thread_mailbox = mailbox.clone();

    let spawned = std::thread::Builder::new()
        .name("fete-panel".into())
        .spawn(move || read_forever(&port, &thread_mailbox));

    if let Err(error) = spawned {
        warn!("panel: could not start the reader thread ({error}) — running without it");
    }

    mailbox
}

/// Open, read until the port goes away, wait, repeat.
fn read_forever(port: &str, mailbox: &Mailbox) {
    // Only complain about a given failure once, or a missing panel writes a
    // line a second into the log all night.
    let mut reported = false;

    warn_if_dialin(port);

    loop {
        mailbox.set_status(LinkStatus::Opening);
        match open_source(port) {
            Ok(reader) => {
                info!("panel: reading {port}");
                mailbox.set_status(LinkStatus::Reading);
                reported = false;
                read_until_closed(reader, mailbox);
                warn!("panel: {port} closed — retrying");
            }
            Err(error) => {
                mailbox.set_status(LinkStatus::Failed(error.to_string()));
                if !reported {
                    warn!("panel: cannot open {port} ({error}) — retrying every second");
                    reported = true;
                }
            }
        }
        std::thread::sleep(RECONNECT_DELAY);
    }
}

/// Open whatever is at this path as a stream of frames.
///
/// Normally a serial port. A named pipe or a plain file is opened directly
/// instead, which is what makes the whole host side testable with no boards on
/// the desk — see `tools/fake-panel.py`. The distinction is made by asking the
/// filesystem rather than by a flag, because there is exactly one right answer
/// for any given path.
fn open_source(port: &str) -> std::io::Result<Box<dyn BufRead + Send>> {
    if is_plain_stream(port) {
        return Ok(Box::new(BufReader::new(File::open(port)?)));
    }

    let mut handle = SerialPort::open(port, 115_200)?;
    if let Err(error) = handle.set_read_timeout(READ_TIMEOUT) {
        warn!("panel: {port} would not take a read timeout ({error})");
    }
    Ok(Box::new(BufReader::new(handle)))
}

/// Whether this path is a pipe or a regular file rather than a device.
fn is_plain_stream(port: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        std::fs::metadata(port).is_ok_and(|meta| {
            let kind = meta.file_type();
            kind.is_fifo() || kind.is_file()
        })
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(port).is_ok_and(|meta| meta.file_type().is_file())
    }
}

/// On macOS a serial device appears twice, and only one of them works here.
///
/// `/dev/tty.*` is the dial-in device: opening it blocks until the far end
/// asserts carrier, which a USB CDC device never does, so the thread would sit
/// in `open` forever with nothing to show for it. `/dev/cu.*` is the callout
/// device and opens immediately. Both names point at the same board, which is
/// what makes this worth catching — the wrong one is not obviously wrong.
fn warn_if_dialin(port: &str) {
    if cfg!(target_os = "macos")
        && let Some(name) = port.strip_prefix("/dev/tty.")
    {
        warn!(
            "panel: {port} is the dial-in device and will block forever — use /dev/cu.{name} instead"
        );
    }
}

/// Pump lines from an open port into the mailbox until the stream ends.
///
/// Takes any reader rather than the port itself, so the decode-and-queue path
/// is testable without a serial device in the room.
fn read_until_closed(mut reader: impl BufRead, mailbox: &Mailbox) {
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            // End of stream: the device went away.
            Ok(0) => return,
            Ok(_) => match parse_line(&line) {
                Ok(Some(frame)) => mailbox.push(frame),
                Ok(None) => {}
                // A garbled line is one dropped frame, not a reason to give up
                // the port — at this baud rate a little line noise is normal.
                Err(error) => debug!("panel: {error}"),
            },
            Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {}
            Err(error) => {
                debug!("panel: read failed ({error})");
                return;
            }
        }
    }
}

/// Serial ports worth offering the operator.
///
/// On macOS the dial-in half of every pair (`/dev/tty.*`) is filtered out:
/// opening one blocks until the far end asserts carrier, which a USB CDC
/// device never does. Listing both would be listing one board twice, with the
/// unusable name first as often as not.
pub fn available() -> Vec<String> {
    let mut ports: Vec<String> = SerialPort::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| !(cfg!(target_os = "macos") && path.starts_with("/dev/tty.")))
        .collect();
    ports.sort();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a reader through the pump and take what lands in the mailbox.
    fn pump(input: &str) -> Vec<Frame> {
        let mailbox = Mailbox::default();
        read_until_closed(BufReader::new(input.as_bytes()), &mailbox);
        mailbox.drain()
    }

    #[test]
    fn queues_frames_in_order() {
        let frames = pump("P 1 1 0\nP 2 2 0\nP 4 3 0\n");
        assert_eq!(
            frames.iter().map(|f| f.buttons).collect::<Vec<_>>(),
            [1, 2, 4]
        );
    }

    #[test]
    fn a_garbled_line_costs_one_frame_and_no_more() {
        let frames = pump("P 1 1 0\nnonsense\nP 4 3 0\n");
        assert_eq!(
            frames.iter().map(|f| f.buttons).collect::<Vec<_>>(),
            [1, 4],
            "a bad line should not end the stream"
        );
    }

    #[test]
    fn firmware_chatter_is_ignored() {
        let frames = pump("# esp-now up\n\nP 3 1 0\n");
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn a_backlog_drops_the_oldest_not_the_newest() {
        // Absolute state means the newest frame is the only one that has to
        // survive; dropping the front of the queue is the safe direction.
        let input: String = (0..MAILBOX_DEPTH + 10)
            .map(|i| format!("P {i:x} {i} 0\n"))
            .collect();
        let frames = pump(&input);
        assert_eq!(frames.len(), MAILBOX_DEPTH);
        assert_eq!(
            frames.last().unwrap().buttons,
            (MAILBOX_DEPTH + 9) as u32,
            "the newest frame must always be the one kept"
        );
    }

    #[test]
    fn an_unopened_link_reports_why() {
        let mailbox = Mailbox::default();
        assert_eq!(mailbox.status(), LinkStatus::Opening);
        mailbox.set_status(LinkStatus::Failed("no such port".into()));
        assert!(matches!(mailbox.status(), LinkStatus::Failed(_)));
    }
}
