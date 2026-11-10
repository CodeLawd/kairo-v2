//! Framing: a little-endian `u32` body length, then the postcard body.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::io::{self, Read, Write};

/// The largest body either side accepts. A slide carries its theme as JSON,
/// which is a few kilobytes; 16 MiB leaves room without letting a corrupt
/// length make the reader allocate gigabytes.
pub const MAX_FRAME_BYTES: u32 = 16 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The length prefix is over [`MAX_FRAME_BYTES`].
    TooLarge(u32),
    /// The body is not a valid message.
    Malformed(String),
    /// The body decoded with bytes left over.
    TrailingBytes(usize),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(length) => {
                write!(
                    f,
                    "frame of {length} bytes is over the {MAX_FRAME_BYTES}-byte limit"
                )
            }
            Self::Malformed(why) => write!(f, "malformed frame: {why}"),
            Self::TrailingBytes(count) => write!(f, "frame has {count} trailing bytes"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl From<DecodeError> for io::Error {
    fn from(error: DecodeError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

/// One framed message, ready to write.
pub fn encode_frame<T: Serialize>(message: &T) -> io::Result<Vec<u8>> {
    let mut framed = postcard::to_extend(message, vec![0; 4])
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let length = u32::try_from(framed.len() - 4)
        .ok()
        .filter(|l| *l <= MAX_FRAME_BYTES)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "message too large"))?;
    framed[..4].copy_from_slice(&length.to_le_bytes());
    Ok(framed)
}

/// Writes one message in a single `write_all`, so concurrent writers that
/// hold the stream's lock never interleave halves of two frames.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    writer.write_all(&encode_frame(message)?)?;
    writer.flush()
}

/// Decodes one body. Every byte must belong to the message.
pub fn decode_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, DecodeError> {
    let (message, rest) =
        postcard::take_from_bytes(body).map_err(|e| DecodeError::Malformed(e.to_string()))?;
    if !rest.is_empty() {
        return Err(DecodeError::TrailingBytes(rest.len()));
    }
    Ok(message)
}

/// Reads one message. End of stream and truncation are `UnexpectedEof`;
/// a bad length or body is `InvalidData`, after which the stream is out of
/// step and must be dropped.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    if length > MAX_FRAME_BYTES {
        return Err(DecodeError::TooLarge(length).into());
    }
    let mut body = vec![0; length as usize];
    reader.read_exact(&mut body)?;
    Ok(decode_body(&body)?)
}

/// Incremental decoder for bytes that arrive in arbitrary pieces. After an
/// error it stays failed: the stream cannot be resynchronised.
#[derive(Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
    failed: bool,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// The next complete message, `Ok(None)` while one is still arriving.
    pub fn next_message<T: DeserializeOwned>(&mut self) -> Result<Option<T>, DecodeError> {
        if self.failed {
            return Err(DecodeError::Malformed("stream already failed".into()));
        }
        let Some(prefix) = self.buffer.get(..4) else {
            return Ok(None);
        };
        let length = u32::from_le_bytes(prefix.try_into().expect("four bytes"));
        if length > MAX_FRAME_BYTES {
            self.failed = true;
            return Err(DecodeError::TooLarge(length));
        }
        let end = 4 + length as usize;
        if self.buffer.len() < end {
            return Ok(None);
        }
        let result = decode_body(&self.buffer[4..end]);
        self.buffer.drain(..end);
        match result {
            Ok(message) => Ok(Some(message)),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
}
