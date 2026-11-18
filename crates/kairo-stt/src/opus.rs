//! Ogg Opus for the Deepgram upload. Raw 16 kHz PCM16 is 256 kbit/s; voice
//! Opus at 24 kbit/s is about a tenth of that, which keeps captions live on a
//! phone hotspot or a congested church network. Deepgram reads Ogg Opus as
//! containerized audio, so the stream carries its own headers (RFC 7845) and
//! the listen URL names no encoding.
//!
//! Each connection is a new logical stream: the two header pages go out with
//! its first audio, and every chunk ends on a page boundary so each WebSocket
//! message stands alone.

use ogg::writing::{PacketWriteEndInfo, PacketWriter};
use opus_rs::{Application, OpusEncoder};

/// The capture rate the relay delivers.
pub const SAMPLE_RATE: u32 = 16_000;
/// 20 ms frames, the Opus default for voice.
const FRAME_SAMPLES: usize = 320;
/// Granule positions count 48 kHz samples whatever the input rate.
const GRANULE_PER_FRAME: u64 = 960;
/// Clear speech for recognition at a tenth of the PCM bitrate.
pub const BITRATE_BPS: i32 = 24_000;
/// Large enough for any 20 ms packet at this bitrate.
const MAX_PACKET: usize = 1_500;

/// Encodes one connection's audio as an Ogg Opus stream.
pub struct OggOpusStream {
    encoder: OpusEncoder,
    writer: PacketWriter<'static, Vec<u8>>,
    serial: u32,
    headers_sent: bool,
    granule: u64,
    pending: Vec<i16>,
    packet: Vec<u8>,
}

impl OggOpusStream {
    pub fn new(serial: u32) -> Result<Self, String> {
        let mut encoder = OpusEncoder::new(SAMPLE_RATE as i32, 1, Application::Voip)
            .map_err(|e| format!("Opus encoder: {e}"))?;
        encoder.bitrate_bps = BITRATE_BPS;
        Ok(Self {
            encoder,
            writer: PacketWriter::new(Vec::new()),
            serial,
            headers_sent: false,
            granule: 0,
            pending: Vec::new(),
            packet: vec![0; MAX_PACKET],
        })
    }

    /// The Ogg bytes for one chunk of little-endian PCM16: headers first on a
    /// new stream, then whole 20 ms packets. A partial frame waits for the
    /// next chunk. Empty when nothing is complete yet.
    pub fn encode(&mut self, pcm: &[u8]) -> Result<Vec<u8>, String> {
        if !self.headers_sent {
            self.write_headers()?;
        }
        self.pending.extend(
            pcm.as_chunks::<2>()
                .0
                .iter()
                .map(|b| i16::from_le_bytes(*b)),
        );
        let frames = self.pending.len() / FRAME_SAMPLES;
        for index in 0..frames {
            let frame = &self.pending[index * FRAME_SAMPLES..(index + 1) * FRAME_SAMPLES];
            let len = self
                .encoder
                .encode_i16(frame, FRAME_SAMPLES, &mut self.packet)
                .map_err(|e| format!("Opus encode: {e}"))?;
            self.granule += GRANULE_PER_FRAME;
            let end = if index + 1 == frames {
                PacketWriteEndInfo::EndPage
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            self.writer
                .write_packet(self.packet[..len].to_vec(), self.serial, end, self.granule)
                .map_err(|e| format!("Ogg page: {e}"))?;
        }
        self.pending.drain(..frames * FRAME_SAMPLES);
        Ok(std::mem::take(self.writer.inner_mut()))
    }

    fn write_headers(&mut self) -> Result<(), String> {
        // OpusHead: version 1, mono, pre-skip in 48 kHz samples, input rate,
        // no gain, channel mapping family 0.
        let pre_skip = (self.encoder.lookahead() as u64 * 48_000 / u64::from(SAMPLE_RATE)) as u16;
        let mut head = b"OpusHead".to_vec();
        head.push(1);
        head.push(1);
        head.extend(pre_skip.to_le_bytes());
        head.extend(SAMPLE_RATE.to_le_bytes());
        head.extend(0i16.to_le_bytes());
        head.push(0);
        let vendor = b"kairo";
        let mut tags = b"OpusTags".to_vec();
        tags.extend((vendor.len() as u32).to_le_bytes());
        tags.extend(vendor);
        tags.extend(0u32.to_le_bytes());
        // Each header packet sits on its own page, as RFC 7845 requires.
        for packet in [head, tags] {
            self.writer
                .write_packet(packet, self.serial, PacketWriteEndInfo::EndPage, 0)
                .map_err(|e| format!("Ogg header: {e}"))?;
        }
        self.headers_sent = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ogg::reading::PacketReader;
    use std::io::Cursor;

    /// 100 ms of a 440 Hz tone, as the relay delivers it.
    fn chunk(offset: usize) -> Vec<u8> {
        (0..1600)
            .flat_map(|i| {
                let t = (offset + i) as f64 / f64::from(SAMPLE_RATE);
                let s = (t * 440.0 * std::f64::consts::TAU).sin() * 8_000.0;
                (s as i16).to_le_bytes()
            })
            .collect()
    }

    #[test]
    fn a_stream_is_valid_ogg_opus_and_a_tenth_of_the_pcm() {
        let mut stream = OggOpusStream::new(7).unwrap();
        let mut ogg = Vec::new();
        let mut pcm_bytes = 0;
        for n in 0..50 {
            let pcm = chunk(n * 1600);
            pcm_bytes += pcm.len();
            let out = stream.encode(&pcm).unwrap();
            assert!(!out.is_empty(), "every 100 ms chunk is a whole page");
            ogg.extend(out);
        }
        let mut reader = PacketReader::new(Cursor::new(ogg.clone()));
        let head = reader.read_packet().unwrap().unwrap();
        assert_eq!(&head.data[..8], b"OpusHead");
        assert_eq!(
            u32::from_le_bytes(head.data[12..16].try_into().unwrap()),
            16_000
        );
        let tags = reader.read_packet().unwrap().unwrap();
        assert_eq!(&tags.data[..8], b"OpusTags");
        let mut packets = 0;
        let mut last_granule = 0;
        while let Some(packet) = reader.read_packet().unwrap() {
            packets += 1;
            last_granule = packet.absgp_page();
        }
        assert_eq!(packets, 250, "5 s of 20 ms frames");
        assert_eq!(last_granule, 250 * GRANULE_PER_FRAME);
        assert!(
            ogg.len() * 6 < pcm_bytes,
            "{} bytes of Ogg for {pcm_bytes} of PCM",
            ogg.len()
        );
    }

    #[test]
    fn the_audio_decodes_back_to_the_same_tone() {
        let mut stream = OggOpusStream::new(3).unwrap();
        let mut ogg = Vec::new();
        for n in 0..20 {
            ogg.extend(stream.encode(&chunk(n * 1600)).unwrap());
        }
        let mut reader = PacketReader::new(Cursor::new(ogg));
        reader.read_packet().unwrap();
        reader.read_packet().unwrap();
        let mut decoder = opus_rs::OpusDecoder::new(SAMPLE_RATE as i32, 1).unwrap();
        let mut out = Vec::new();
        while let Some(packet) = reader.read_packet().unwrap() {
            let mut frame = vec![0f32; FRAME_SAMPLES];
            let n = decoder
                .decode(&packet.data, FRAME_SAMPLES, &mut frame)
                .unwrap();
            out.extend_from_slice(&frame[..n]);
        }
        assert_eq!(out.len(), 20 * 1600);
        // Past the codec's start-up, the tone's level survives: the RMS of an
        // 8000-peak sine is about 0.17 of full scale.
        let tail = &out[out.len() / 2..];
        let rms = (tail.iter().map(|s| s * s).sum::<f32>() / tail.len() as f32).sqrt();
        assert!((0.12..0.22).contains(&rms), "decoded rms {rms}");
    }

    #[test]
    fn a_partial_frame_waits_for_the_next_chunk() {
        let mut stream = OggOpusStream::new(1).unwrap();
        let headers = stream.encode(&chunk(0)[..200]).unwrap();
        let head_only = headers.len();
        assert!(head_only > 0, "headers go out first");
        let more = stream.encode(&chunk(100)[..600]).unwrap();
        assert!(!more.is_empty(), "a full frame is now available");
    }
}
