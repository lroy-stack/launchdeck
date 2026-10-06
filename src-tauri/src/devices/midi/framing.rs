//! `MessageFramer`: the bytes an input callback receives, as whole MIDI
//! messages. A system may hand over several messages in one call, or a
//! SysEx in pieces; a device reads one message at a time. Pure, no protocol
//! knowledge.
//!
//! The rules are MIDI 1.0's, with two choices: running status is not
//! supported (a data byte with no status byte before it in the same message
//! is dropped), and real-time bytes (`F8h`-`FFh`: clock, start, active
//! sensing, …) are dropped wherever they appear, since they may sit inside
//! another message and no device LaunchDeck drives sends them on purpose.

const STATUS: u8 = 0x80;
const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;
const REAL_TIME: u8 = 0xF8;

/// The longest SysEx kept, `F0h` and `F7h` included; a longer one is
/// dropped whole. The replies LaunchDeck reads are a few bytes long.
const MAX_SYSEX: usize = 1024;

#[derive(Debug, Default)]
enum Reading {
    /// Between messages.
    #[default]
    Idle,
    /// A message of known length, `bytes` so far.
    Short { bytes: Vec<u8>, length: usize },
    /// A SysEx until its `F7h`; `None` once it grew too long.
    SysEx(Option<Vec<u8>>),
}

/// Keeps an unfinished message from one callback to the next.
#[derive(Debug, Default)]
pub(crate) struct MessageFramer {
    reading: Reading,
}

impl MessageFramer {
    /// The whole messages these bytes complete, in order.
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        bytes.iter().filter_map(|&b| self.push(b)).collect()
    }

    fn push(&mut self, byte: u8) -> Option<Vec<u8>> {
        if byte >= REAL_TIME {
            return None;
        }
        if byte == SYSEX_END {
            return match std::mem::take(&mut self.reading) {
                Reading::SysEx(Some(mut message)) => {
                    message.push(byte);
                    Some(message)
                }
                _ => None,
            };
        }
        if byte >= STATUS {
            // A status byte ends whatever was unfinished, which is dropped.
            self.reading = Reading::Idle;
            match (byte, data_bytes(byte)) {
                (SYSEX_START, _) => self.reading = Reading::SysEx(Some(vec![byte])),
                (_, Some(0)) => return Some(vec![byte]),
                (_, Some(n)) => {
                    self.reading = Reading::Short {
                        bytes: vec![byte],
                        length: n + 1,
                    }
                }
                (_, None) => {}
            }
            return None;
        }
        self.data(byte)
    }

    fn data(&mut self, byte: u8) -> Option<Vec<u8>> {
        match &mut self.reading {
            Reading::Idle | Reading::SysEx(None) => None,
            Reading::SysEx(Some(message)) => {
                // Room is kept for the closing F7h.
                if message.len() + 1 < MAX_SYSEX {
                    message.push(byte);
                } else {
                    self.reading = Reading::SysEx(None);
                }
                None
            }
            Reading::Short { bytes, length } => {
                bytes.push(byte);
                if bytes.len() < *length {
                    return None;
                }
                let message = std::mem::take(bytes);
                self.reading = Reading::Idle;
                Some(message)
            }
        }
    }
}

/// How many data bytes follow a status byte; `None` for the SysEx bytes
/// and the undefined `F4h` and `F5h`.
fn data_bytes(status: u8) -> Option<usize> {
    match status {
        // Note off and on, polyphonic pressure, control change, pitch bend.
        0x80..=0xBF | 0xE0..=0xEF => Some(2),
        // Program change, channel pressure.
        0xC0..=0xDF => Some(1),
        // Time code quarter frame, song select; song position.
        0xF1 | 0xF3 => Some(1),
        0xF2 => Some(2),
        // Tune request.
        0xF6 => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(calls: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut framer = MessageFramer::default();
        calls.iter().flat_map(|bytes| framer.feed(bytes)).collect()
    }

    const ENTER: [u8; 9] = [0xF0, 0x00, 0x20, 0x29, 0x02, 0x0D, 0x0E, 0x01, 0xF7];

    #[test]
    fn two_messages_in_one_callback_are_two_messages() {
        assert_eq!(
            frame(&[&[0x90, 0x0B, 0x7F, 0xB0, 0x5B, 0x00]]),
            [vec![0x90, 0x0B, 0x7F], vec![0xB0, 0x5B, 0x00]]
        );
        let mut both = ENTER.to_vec();
        both.extend([0x90, 0x0B, 0x00]);
        assert_eq!(frame(&[&both]), [ENTER.to_vec(), vec![0x90, 0x0B, 0x00]]);
    }

    #[test]
    fn a_sysex_split_over_three_callbacks_is_one_message() {
        let mut framer = MessageFramer::default();
        assert!(framer.feed(&ENTER[..3]).is_empty());
        assert!(framer.feed(&ENTER[3..7]).is_empty());
        assert_eq!(framer.feed(&ENTER[7..]), [ENTER.to_vec()]);
    }

    #[test]
    fn a_short_message_split_over_callbacks_is_one_message() {
        assert_eq!(
            frame(&[&[0x90], &[0x0B], &[0x7F]]),
            [vec![0x90, 0x0B, 0x7F]]
        );
    }

    #[test]
    fn stray_data_bytes_are_dropped() {
        assert_eq!(
            frame(&[&[0x0B, 0x7F, 0x90, 0x0B, 0x7F, 0x55]]),
            [vec![0x90, 0x0B, 0x7F]]
        );
        assert_eq!(
            frame(&[&[0xF7, 0x01]]),
            Vec::<Vec<u8>>::new(),
            "a stray end"
        );
    }

    #[test]
    fn running_status_is_not_supported() {
        assert_eq!(
            frame(&[&[0x90, 0x0B, 0x7F, 0x0C, 0x7F]]),
            [vec![0x90, 0x0B, 0x7F]]
        );
    }

    #[test]
    fn real_time_bytes_are_dropped_even_inside_a_message() {
        let mut clocked = ENTER.to_vec();
        clocked.insert(4, 0xF8);
        clocked.insert(1, 0xFE);
        assert_eq!(frame(&[&clocked]), [ENTER.to_vec()]);
        assert_eq!(
            frame(&[&[0xF8, 0x90, 0xF8, 0x0B, 0xFF, 0x7F, 0xFA]]),
            [vec![0x90, 0x0B, 0x7F]]
        );
    }

    #[test]
    fn a_status_byte_drops_the_unfinished_message() {
        assert_eq!(
            frame(&[&[0xF0, 0x00, 0x20], &[0x90, 0x0B, 0x7F, 0x29, 0xF7]]),
            [vec![0x90, 0x0B, 0x7F]]
        );
        assert_eq!(
            frame(&[&[0x90, 0x0B, 0xB0, 0x5B, 0x7F]]),
            [vec![0xB0, 0x5B, 0x7F]]
        );
    }

    #[test]
    fn messages_have_their_own_lengths() {
        assert_eq!(
            frame(&[&[0xD0, 0x40, 0xC0, 0x05, 0xE0, 0x00, 0x40, 0xF6, 0xF2, 0x01, 0x02]]),
            [
                vec![0xD0, 0x40],
                vec![0xC0, 0x05],
                vec![0xE0, 0x00, 0x40],
                vec![0xF6],
                vec![0xF2, 0x01, 0x02],
            ]
        );
        assert_eq!(
            frame(&[&[0xF4, 0x01, 0xF5]]),
            Vec::<Vec<u8>>::new(),
            "undefined"
        );
    }

    #[test]
    fn an_overlong_sysex_is_dropped_whole() {
        let mut long = vec![0xF0];
        long.extend(std::iter::repeat_n(0x01, MAX_SYSEX));
        long.push(0xF7);
        long.extend([0x90, 0x0B, 0x7F]);
        assert_eq!(frame(&[&long]), [vec![0x90, 0x0B, 0x7F]]);

        let mut longest = vec![0xF0];
        longest.extend(std::iter::repeat_n(0x01, MAX_SYSEX - 2));
        longest.push(0xF7);
        assert_eq!(frame(&[&longest]), [longest.clone()], "within the limit");
    }
}
