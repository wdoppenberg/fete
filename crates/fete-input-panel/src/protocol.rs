//! The wire format between the receiver board and this process.
//!
//! Lines of ASCII, one frame per line. Binary framing would be smaller and
//! this is neither large nor frequent enough for that to matter; being able to
//! open the port in any terminal and read what the panel is saying is worth
//! more on a dark stage than the bytes are.
//!
//! ```text
//! P 0000000a 4213 12
//! │ │        │    └── milliseconds since the receiver last heard the panel
//! │ │        └─────── transmitter sequence, for spotting lost radio packets
//! │ └──────────────── button bitmask in hex, bit 0 is button 0
//! └────────────────── frame kind
//! ```
//!
//! Anything starting with `#` is a comment and ignored, so the firmware can log
//! to the same port during bring-up without confusing the host.
//!
//! The mask is **absolute state, not edges**. Over a radio in a room full of
//! people, a dropped "button 3 toggled" is a panel that stays wrong until
//! someone presses it again; a dropped "these buttons are down" is corrected by
//! the next frame a few milliseconds later.

use std::fmt;

/// One decoded line from the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// Bit `n` set means button `n` is currently held.
    pub buttons: u32,
    /// Wrapping counter from the transmitter's ESP-NOW packets.
    ///
    /// The receiver repeats a value when it writes another USB frame before a
    /// new radio packet arrives. Forward jumps reveal radio packet loss.
    pub seq: u16,
    /// How long ago the receiver last heard from the panel, in milliseconds.
    ///
    /// This is the far half of the link: the host can tell that the receiver is
    /// alive simply by lines arriving, but only the receiver knows whether the
    /// board out on the screen is still talking to it.
    pub panel_age_ms: u32,
}

/// Why a line could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The leading token was not a frame kind this version knows.
    UnknownKind(String),
    /// A `P` frame did not carry mask, sequence and age.
    WrongFieldCount(usize),
    /// One of the three numbers did not parse.
    BadField(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownKind(kind) => write!(f, "unknown frame kind `{kind}`"),
            Self::WrongFieldCount(n) => write!(f, "expected 3 fields after `P`, got {n}"),
            Self::BadField(name) => write!(f, "could not parse the {name} field"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Decode one line.
///
/// `Ok(None)` is a line that is valid but carries nothing — a comment, or
/// blank. Those are common enough during firmware bring-up that treating them
/// as errors would bury the real ones.
pub fn parse_line(line: &str) -> Result<Option<Frame>, ParseError> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }

    let mut fields = line.split_whitespace();
    let kind = fields.next().unwrap_or_default();
    if kind != "P" {
        return Err(ParseError::UnknownKind(kind.to_string()));
    }

    let rest: Vec<&str> = fields.collect();
    let [mask, seq, age] = rest[..] else {
        return Err(ParseError::WrongFieldCount(rest.len()));
    };

    Ok(Some(Frame {
        buttons: u32::from_str_radix(mask, 16).map_err(|_| ParseError::BadField("button mask"))?,
        seq: seq.parse().map_err(|_| ParseError::BadField("sequence"))?,
        panel_age_ms: age.parse().map_err(|_| ParseError::BadField("panel age"))?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_frame() {
        let frame = parse_line("P 0000000a 4213 12").unwrap().unwrap();
        assert_eq!(frame.buttons, 0b1010);
        assert_eq!(frame.seq, 4213);
        assert_eq!(frame.panel_age_ms, 12);
    }

    #[test]
    fn tolerates_ragged_whitespace() {
        let frame = parse_line("  P   ff  1   0  \r\n").unwrap().unwrap();
        assert_eq!(frame.buttons, 0xff);
    }

    #[test]
    fn comments_and_blanks_carry_nothing() {
        assert_eq!(parse_line("# booting").unwrap(), None);
        assert_eq!(parse_line("   ").unwrap(), None);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert!(matches!(
            parse_line("Q 1 2 3"),
            Err(ParseError::UnknownKind(_))
        ));
        assert!(matches!(
            parse_line("P 1 2"),
            Err(ParseError::WrongFieldCount(2))
        ));
        assert!(matches!(
            parse_line("P zz 2 3"),
            Err(ParseError::BadField("button mask"))
        ));
    }

    /// Byte-for-byte what `firmware/src/receiver/main.cpp` emits, from a
    /// `printf("P %08lx %u %lu\n", ...)` compiled and run for the purpose. If
    /// the firmware's format string changes, this is what should break.
    #[test]
    fn accepts_what_the_receiver_firmware_actually_prints() {
        let cases = [
            ("P 00000000 65535 5", 0, 65535, 5),
            ("P 0000000a 1 999999", 0b1010, 1, 999_999),
            ("P 80000000 2 0", 1 << 31, 2, 0),
            ("P 000000ff 3 1500", 0xff, 3, 1500),
        ];
        for (line, buttons, seq, age) in cases {
            let frame = parse_line(line)
                .unwrap_or_else(|e| panic!("firmware line {line:?} rejected: {e}"))
                .unwrap_or_else(|| panic!("firmware line {line:?} carried nothing"));
            assert_eq!(
                (frame.buttons, frame.seq, frame.panel_age_ms),
                (buttons, seq, age)
            );
        }
    }

    #[test]
    fn the_top_button_survives_the_round_trip() {
        // Bit 31 in a u32 mask is the one an `i32` parse would turn negative.
        let frame = parse_line("P 80000000 0 0").unwrap().unwrap();
        assert_eq!(frame.buttons, 1 << 31);
    }
}
