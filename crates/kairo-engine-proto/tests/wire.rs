//! Framing round trips and a decoder fuzz over malformed frames.

use kairo_engine_proto::*;

fn sample_commands() -> Vec<ToEngine> {
    let theme = Json(r##"{"background":{"type":"color","color":"#000"}}"##.into());
    vec![
        ToEngine::Hello(Hello {
            protocol: PROTOCOL_VERSION,
            token: "t0k3n".into(),
        }),
        ToEngine::Ping { nonce: u64::MAX },
        ToEngine::Query {
            id: 9,
            query: Query::State,
        },
        ToEngine::Command {
            seq: 1,
            command: Command::OpenSurface(SurfaceSpec {
                output: "out-1".into(),
                label: "Main screen".into(),
                sink: SinkSpec::Display(DisplaySink {
                    binding: DisplayBinding {
                        display_id: Some(7),
                        label: "Sceptre Z27".into(),
                        size: Some((1920, 1080)),
                    },
                    rehearsal: true,
                    frame_height: 1080,
                }),
            }),
        },
        ToEngine::Command {
            seq: 2,
            command: Command::ShowSlide {
                output: "out-1".into(),
                source: "out-1".into(),
                slide: SlideContent {
                    reference: "John 3:16".into(),
                    text: "For God so loved the world — ẹ̀ṣẹ́".into(),
                    theme: theme.clone(),
                    colored_lines: Some(Json("[]".into())),
                },
                instant: false,
            },
        },
        ToEngine::Command {
            seq: 3,
            command: Command::WebPointer {
                output: "out-1".into(),
                point: Some((0.25, -1.5)),
            },
        },
        ToEngine::Command {
            seq: 4,
            command: Command::Configure {
                output: "out-1".into(),
                config: SurfaceConfig {
                    transition_ms: 400.0,
                    video_layout: Some(Json("{}".into())),
                    ..SurfaceConfig::default()
                },
            },
        },
        ToEngine::Command {
            seq: 5,
            command: Command::Shutdown,
        },
    ]
}

#[test]
fn every_sample_round_trips_through_read_and_the_incremental_decoder() {
    let messages = sample_commands();
    let mut stream = Vec::new();
    for message in &messages {
        write_frame(&mut stream, message).unwrap();
    }
    let mut reader = stream.as_slice();
    for message in &messages {
        assert_eq!(&read_frame::<ToEngine>(&mut reader).unwrap(), message);
    }
    assert_eq!(
        read_frame::<ToEngine>(&mut reader).unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );

    // Byte at a time: nothing decodes early, everything decodes once.
    let mut decoder = FrameDecoder::default();
    let mut decoded = Vec::new();
    for byte in &stream {
        decoder.push(std::slice::from_ref(byte));
        while let Some(message) = decoder.next_message::<ToEngine>().unwrap() {
            decoded.push(message);
        }
    }
    assert_eq!(decoded, messages);
}

#[test]
fn engine_messages_round_trip() {
    let messages = vec![
        FromEngine::Welcome(Welcome {
            protocol: 1,
            engine_version: "0.1.0".into(),
            pid: 42,
        }),
        FromEngine::Rejected {
            reason: "protocol version mismatch".into(),
        },
        FromEngine::Event(Event::FrameCommitted {
            output: "a".into(),
            seq: 3,
        }),
        FromEngine::Event(Event::SurfaceStatus {
            output: "a".into(),
            status: SurfaceStatus::Parked {
                reason: "Display not connected".into(),
            },
        }),
        FromEngine::Reply {
            id: 1,
            reply: Reply::DocumentPlayback(None),
        },
    ];
    for message in messages {
        let frame = encode_frame(&message).unwrap();
        assert_eq!(
            read_frame::<FromEngine>(&mut frame.as_slice()).unwrap(),
            message
        );
    }
}

#[test]
fn oversize_truncated_and_trailing_frames_are_errors() {
    let oversize = (MAX_FRAME_BYTES + 1).to_le_bytes();
    let error = read_frame::<ToEngine>(&mut oversize.as_slice()).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    let mut frame = encode_frame(&ToEngine::Ping { nonce: 7 }).unwrap();
    frame.pop();
    assert_eq!(
        read_frame::<ToEngine>(&mut frame.as_slice())
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::UnexpectedEof
    );

    let mut frame = encode_frame(&ToEngine::Ping { nonce: 7 }).unwrap();
    frame.push(0);
    let length = (frame.len() - 4) as u32;
    frame[..4].copy_from_slice(&length.to_le_bytes());
    assert_eq!(decode_error(&frame), Some(DecodeError::TrailingBytes(1)),);

    // An empty body is never a message.
    assert!(matches!(
        decode_error(&0u32.to_le_bytes()),
        Some(DecodeError::Malformed(_))
    ));

    // A failed decoder stays failed: the stream is out of step.
    let mut decoder = FrameDecoder::default();
    decoder.push(&oversize);
    assert!(decoder.next_message::<ToEngine>().is_err());
    decoder.push(&encode_frame(&ToEngine::Ping { nonce: 1 }).unwrap());
    assert!(decoder.next_message::<ToEngine>().is_err());
}

fn decode_error(frame: &[u8]) -> Option<DecodeError> {
    let mut decoder = FrameDecoder::default();
    decoder.push(frame);
    decoder.next_message::<ToEngine>().err()
}

/// xorshift64*: deterministic, so a failing case reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Feeds the decoders random bytes and mutated valid frames. Each must
/// return a message or an error: never panic, hang or allocate past the
/// frame limit.
#[test]
fn fuzz_decoder_with_malformed_frames() {
    let valid: Vec<Vec<u8>> = sample_commands()
        .iter()
        .map(|m| encode_frame(m).unwrap())
        .collect();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut decoded = 0;
    let mut rejected = 0;
    for case in 0..200_000 {
        let mut bytes = if case % 4 == 0 {
            let length = rng.below(64);
            (0..length).map(|_| rng.next() as u8).collect::<Vec<_>>()
        } else {
            valid[rng.below(valid.len())].clone()
        };
        if case % 4 != 0 {
            for _ in 0..=rng.below(4) {
                match rng.below(4) {
                    0 if !bytes.is_empty() => {
                        let at = rng.below(bytes.len());
                        bytes[at] ^= 1 << rng.below(8);
                    }
                    1 if !bytes.is_empty() => {
                        bytes.truncate(rng.below(bytes.len()));
                    }
                    2 => {
                        let at = rng.below(bytes.len() + 1);
                        bytes.insert(at, rng.next() as u8);
                    }
                    _ if bytes.len() > 4 => {
                        // Keep the prefix honest about the body, so the
                        // body decoder sees the mutation.
                        let length = (bytes.len() - 4) as u32;
                        bytes[..4].copy_from_slice(&length.to_le_bytes());
                    }
                    _ => {}
                }
            }
        }
        match read_frame::<ToEngine>(&mut bytes.as_slice()) {
            Ok(_) => decoded += 1,
            Err(_) => rejected += 1,
        }
        let mut decoder = FrameDecoder::default();
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let take = 1 + rng.below(rest.len());
            decoder.push(&rest[..take]);
            rest = &rest[take..];
            if decoder.next_message::<FromEngine>().is_err() {
                break;
            }
        }
    }
    assert!(
        decoded > 0 && rejected > 0,
        "decoded {decoded}, rejected {rejected}"
    );
}
