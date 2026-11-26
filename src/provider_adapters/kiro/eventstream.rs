//! AWS EventStream binary frame decoding for Kiro's CodeWhisperer runtime.
//!
//! Frames are: 12-byte prelude (total length, headers length, prelude CRC32),
//! headers, JSON payload, and a trailing message CRC32 — all big-endian, CRC32
//! IEEE. Ported from OmniRoute's `parseEventFrame`.
use serde_json::Value;

const PRELUDE_BYTES: usize = 12;
const CRC_BYTES: usize = 4;
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const MAX_HEADERS_BYTES: usize = 64 * 1024;
const MAX_HEADER_NAME_BYTES: usize = 1024;

#[derive(Debug)]
pub(super) struct EventFrame {
    pub event_type: String,
    pub payload: Option<Value>,
}

/// Incremental frame parser: feed bytes, drain complete frames.
pub(super) struct FrameParser {
    buffer: Vec<u8>,
}

impl FrameParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64 * 1024),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        if self.buffer.len() + chunk.len() > MAX_FRAME_BYTES * 2 {
            self.buffer.clear();
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    /// Extracts every complete frame currently buffered.
    pub fn drain(&mut self) -> Vec<EventFrame> {
        let mut frames = Vec::new();
        loop {
            match parse_frame(&self.buffer) {
                FrameParse::NeedMore => break,
                FrameParse::Invalid => {
                    // Resync: drop one byte and retry so a corrupt frame cannot
                    // wedge the stream forever.
                    if self.buffer.is_empty() {
                        break;
                    }
                    self.buffer.remove(0);
                }
                FrameParse::Frame { consumed, frame } => {
                    self.buffer.drain(..consumed);
                    if let Some(frame) = frame {
                        frames.push(frame);
                    }
                }
            }
        }
        frames
    }
}

enum FrameParse {
    NeedMore,
    Invalid,
    Frame {
        consumed: usize,
        frame: Option<EventFrame>,
    },
}

fn parse_frame(buffer: &[u8]) -> FrameParse {
    if buffer.len() < PRELUDE_BYTES + CRC_BYTES {
        return FrameParse::NeedMore;
    }
    let total_length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let headers_length = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]) as usize;
    let length_out_of_range =
        !(PRELUDE_BYTES + CRC_BYTES..=MAX_FRAME_BYTES).contains(&total_length);
    if length_out_of_range || headers_length > MAX_HEADERS_BYTES || total_length > buffer.len() {
        return if length_out_of_range {
            FrameParse::Invalid
        } else {
            FrameParse::NeedMore
        };
    }
    let frame = &buffer[..total_length];
    let prelude_crc = u32::from_be_bytes([frame[8], frame[9], frame[10], frame[11]]);
    if crc32(&frame[..8]) != prelude_crc {
        return FrameParse::Invalid;
    }
    let message_crc = u32::from_be_bytes([
        frame[total_length - 4],
        frame[total_length - 3],
        frame[total_length - 2],
        frame[total_length - 1],
    ]);
    if crc32(&frame[..total_length - CRC_BYTES]) != message_crc {
        return FrameParse::Invalid;
    }

    let header_end = PRELUDE_BYTES + headers_length;
    if header_end > total_length - CRC_BYTES {
        return FrameParse::Invalid;
    }
    let mut event_type = String::new();
    let mut offset = PRELUDE_BYTES;
    while offset < header_end {
        let name_len = frame[offset] as usize;
        offset += 1;
        if name_len == 0 || name_len > MAX_HEADER_NAME_BYTES || offset + name_len >= header_end {
            return FrameParse::Invalid;
        }
        let name = match std::str::from_utf8(&frame[offset..offset + name_len]) {
            Ok(name) => name,
            Err(_) => return FrameParse::Invalid,
        };
        offset += name_len;
        let header_type = frame[offset];
        offset += 1;
        match header_type {
            // String header: u16 length + UTF-8 bytes.
            7 => {
                if offset + 2 > header_end {
                    return FrameParse::Invalid;
                }
                let value_len = u16::from_be_bytes([frame[offset], frame[offset + 1]]) as usize;
                offset += 2;
                if offset + value_len > header_end {
                    return FrameParse::Invalid;
                }
                if name == ":event-type" {
                    event_type =
                        String::from_utf8_lossy(&frame[offset..offset + value_len]).into_owned();
                }
                offset += value_len;
            }
            // Bool true/false headers carry no payload.
            0 | 1 => {}
            // Other header types are skipped by length where possible; Kiro
            // only emits string headers in practice.
            _ => return FrameParse::Invalid,
        }
    }

    let payload_start = header_end;
    let payload_end = total_length - CRC_BYTES;
    let payload = if payload_end > payload_start {
        let text = String::from_utf8_lossy(&frame[payload_start..payload_end]);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            match serde_json::from_str::<Value>(trimmed) {
                Ok(value) => Some(value),
                Err(_) => Some(serde_json::json!({ "raw": trimmed })),
            }
        }
    } else {
        None
    };

    FrameParse::Frame {
        consumed: total_length,
        frame: Some(EventFrame {
            event_type,
            payload,
        }),
    }
}

/// CRC32 (IEEE 802.3, reflected) used by AWS EventStream framing.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32_test(data: &[u8]) -> u32 {
        crc32(data)
    }

    #[test]
    fn crc32_matches_known_vector() {
        // CRC-32/ISO-HDLC check value for "123456789" is 0xCBF43926.
        assert_eq!(crc32_test(b"123456789"), 0xCBF4_3926);
    }

    fn build_frame(event_type: &str, payload: &str) -> Vec<u8> {
        let name = b":event-type";
        let value = event_type.as_bytes();
        let mut headers = Vec::new();
        headers.push(name.len() as u8);
        headers.extend_from_slice(name);
        headers.push(7u8);
        headers.extend_from_slice(&(value.len() as u16).to_be_bytes());
        headers.extend_from_slice(value);
        let headers_length = headers.len() as u32;
        let payload_bytes = payload.as_bytes();
        let total_length =
            (PRELUDE_BYTES + headers_length as usize + payload_bytes.len() + CRC_BYTES) as u32;
        let mut frame = Vec::new();
        frame.extend_from_slice(&total_length.to_be_bytes());
        frame.extend_from_slice(&headers_length.to_be_bytes());
        frame.extend_from_slice(&crc32(&frame[..8]).to_be_bytes());
        frame.extend_from_slice(&headers);
        frame.extend_from_slice(payload_bytes);
        let crc = crc32(&frame);
        frame.extend_from_slice(&crc.to_be_bytes());
        frame
    }

    #[test]
    fn parses_a_well_formed_frame() {
        let mut bytes = build_frame("assistantResponseEvent", r#"{"content":"hi"}"#);
        let mut parser = FrameParser::new();
        parser.push(&bytes);
        let frames = parser.drain();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event_type, "assistantResponseEvent");
        assert_eq!(
            frames[0].payload.as_ref().and_then(|p| p.get("content")),
            Some(&Value::String("hi".to_owned()))
        );
        bytes.clear();
    }

    #[test]
    fn rejects_corrupt_crc_and_resyncs() {
        let mut bytes = build_frame("messageStopEvent", "{}");
        // Corrupt the message CRC.
        let n = bytes.len();
        bytes[n - 1] ^= 0xff;
        let mut parser = FrameParser::new();
        parser.push(&bytes);
        assert!(parser.drain().is_empty());
    }

    #[test]
    fn handles_frames_split_across_chunks() {
        let bytes = build_frame("codeEvent", r#"{"content":"x"}"#);
        let mut parser = FrameParser::new();
        parser.push(&bytes[..10]);
        assert!(parser.drain().is_empty());
        parser.push(&bytes[10..]);
        let frames = parser.drain();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event_type, "codeEvent");
    }
}
