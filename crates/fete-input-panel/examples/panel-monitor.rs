//! Watch a panel without starting the show.
//!
//! ```sh
//! cargo run -p fete-input-panel --example panel-monitor            # list ports
//! cargo run -p fete-input-panel --example panel-monitor /dev/tty.usbmodem101
//! ```
//!
//! Prints every frame the receiver sends, and flags gaps in the sequence
//! numbers. This is the tool for bringing up firmware: it answers "is the
//! board saying anything, and is the radio dropping it" without a projector,
//! a GPU or a dark room being involved.

use std::io::Write;
use std::time::{Duration, Instant};

use fete_input_panel::{LinkStatus, Mailbox, available, spawn_reader};

fn main() {
    let Some(port) = std::env::args().nth(1) else {
        println!("usage: panel-monitor <port>\n\nports on this machine:");
        for port in available() {
            println!("  {port}");
        }
        return;
    };

    println!("watching {port} — ctrl-c to stop");
    let mailbox: Mailbox = spawn_reader(port);

    let mut last_seq: Option<u16> = None;
    let mut seen = 0u64;
    let mut dropped = 0u64;
    let mut last_report = Instant::now();

    loop {
        for frame in mailbox.drain() {
            if let Some(last) = last_seq {
                dropped += u64::from(frame.seq.wrapping_sub(last).saturating_sub(1));
            }
            last_seq = Some(frame.seq);
            seen += 1;

            if frame.buttons != 0 {
                println!(
                    "buttons {:08b}  seq {}  panel {} ms ago",
                    frame.buttons, frame.seq, frame.panel_age_ms
                );
            }
        }

        // A steady line even when nobody is pressing anything, so silence is
        // distinguishable from a dead link.
        if last_report.elapsed() >= Duration::from_secs(2) {
            // The status matters more than the counts when the counts are zero:
            // "0 frames" and "could not open the port" look identical
            // otherwise, and on macOS the usual cause is the tty/cu mix-up.
            match mailbox.status() {
                LinkStatus::Reading => println!("-- {seen} frames, {dropped} dropped"),
                LinkStatus::Opening => println!("-- opening..."),
                LinkStatus::Failed(why) => println!("-- cannot open the port: {why}"),
            }
            last_report = Instant::now();
            // Redirected to a file, stdout is block-buffered, and a diagnostic
            // whose output only appears once you stop it is not much of a
            // diagnostic.
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
