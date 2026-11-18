use std::{
    collections::VecDeque,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
    time::Duration,
};

use futures_util::future::BoxFuture;
use kairo_domain::cloud::ManagedRequest;
use kairo_scripture::{
    live_detector::{LiveCitationDetector, TranscriptEvent, TranscriptKind, UnresolvedTranscript},
    model_detector::{
        DetectionType, ManagedDetectorTransport, ManagedModelDetector, ModelConfig, ModelError,
        ModelOutcome, ModelProvider, ModelQueue, ModelWork, model_request,
    },
};
use serde_json::{Value, json};
use tokio::sync::watch;

#[derive(Default)]
struct Transport {
    replies: Mutex<VecDeque<Result<Value, ModelError>>>,
    requests: Mutex<Vec<ManagedRequest>>,
}

impl ManagedDetectorTransport for &Transport {
    fn request(&self, request: ManagedRequest) -> BoxFuture<'_, Result<Value, ModelError>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request);
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("recorded response")
        })
    }
}

struct Pending;

impl ManagedDetectorTransport for Pending {
    fn request(&self, _: ManagedRequest) -> BoxFuture<'_, Result<Value, ModelError>> {
        Box::pin(std::future::pending())
    }
}

fn work(text: &str) -> ModelWork {
    ModelWork {
        transcript: UnresolvedTranscript {
            text: text.into(),
            received_at_ms: 500,
        },
        generation: 0,
    }
}

fn deepseek(content: &str) -> Value {
    json!({"choices": [{"message": {"content": content}}]})
}

fn anthropic(content: &str) -> Value {
    json!({"content": [{"type": "thinking", "thinking": "ignored"}, {"type": "text", "text": content}]})
}

#[tokio::test(start_paused = true)]
async fn switches_provider_once_and_retries_malformed_json() {
    let transport = Transport::default();
    let reason = "DeepSeek has not been configured";
    transport.replies.lock().unwrap().extend([
        Err(ModelError::gateway(Some(503), reason.into())),
        Err(ModelError::gateway(Some(503), reason.into())),
        Err(ModelError::gateway(Some(503), reason.into())),
        Ok(anthropic("malformed")),
        Ok(anthropic("[]")),
    ]);
    let mut detector = ManagedModelDetector::new(
        &transport,
        ModelConfig {
            model: Some("custom-deepseek".into()),
            ..ModelConfig::default()
        },
    );
    let (_sender, cancel) = watch::channel(false);
    assert_eq!(
        detector.run(&work("Scripture says"), cancel).await,
        ModelOutcome::Parsed(vec![])
    );
    let requests = transport.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests[0].body.as_ref().unwrap()["model"],
        "custom-deepseek"
    );
    assert_eq!(
        requests[3].body.as_ref().unwrap()["model"],
        "claude-haiku-4-5"
    );
    assert_eq!(detector.stats().total_calls, 2);
    assert_eq!(detector.stats().errors, 0);
}

#[tokio::test(start_paused = true)]
async fn an_empty_answer_means_no_references_not_a_failure() {
    for (provider, reply) in [
        (ModelProvider::Deepseek, deepseek("")),
        (ModelProvider::Deepseek, deepseek("  \n")),
        (ModelProvider::Anthropic, anthropic("")),
    ] {
        let transport = Transport::default();
        transport.replies.lock().unwrap().push_back(Ok(reply));
        let config = ModelConfig {
            provider,
            ..ModelConfig::default()
        };
        let mut detector = ManagedModelDetector::new(&transport, config);
        let (_sender, cancel) = watch::channel(false);
        assert_eq!(
            detector.run(&work("Scripture says"), cancel).await,
            ModelOutcome::Parsed(vec![])
        );
        assert_eq!(detector.stats().errors, 0);
        assert_eq!(transport.requests.lock().unwrap().len(), 1, "no retry");
    }
}

#[tokio::test(start_paused = true)]
async fn timeouts_cancel_network_and_local_fast_path_keeps_running() {
    let mut model = ManagedModelDetector::new(Pending, ModelConfig::default());
    let (sender, cancel) = watch::channel(false);
    let pending = work("The Bible says");
    let future = model.run(&pending, cancel.clone());
    tokio::pin!(future);
    let mut local = LiveCitationDetector::default();
    tokio::select! {
        _ = &mut future => panic!("pending transport completed"),
        _ = tokio::time::sleep(Duration::from_millis(1)) => {
            let output = local.process(TranscriptEvent {
                at_ms: 501, kind: TranscriptKind::Interim,
                text: "Romans 8:28", audio_epoch: None,
            });
            assert_eq!(output.len(), 1);
            assert_eq!(output[0].emitted_at_ms, 501);
            assert_ne!(local.generation(), pending.generation);

            let mut samples = Vec::with_capacity(200);
            for index in 0..200 {
                // Distinct interim strings exercise parsing even when the
                // same canonical verse is already in the dedup cache.
                let text = format!("Let us read Romans 8:28. Interim revision {index}.");
                let start = std::time::Instant::now();
                local.process(TranscriptEvent {
                    at_ms: 502 + index,
                    kind: TranscriptKind::Interim,
                    text: &text,
                    audio_epoch: None,
                });
                samples.push(start.elapsed());
            }
            samples.sort_unstable();
            eprintln!("local event p99 with pending managed request: {:?}", samples[197]);
        }
    }
    sender.send(true).unwrap();
    assert_eq!(future.await, ModelOutcome::Cancelled);

    let (_sender, cancel) = watch::channel(false);
    let mut model = ManagedModelDetector::new(Pending, ModelConfig::default());
    let start = tokio::time::Instant::now();
    let result = model.run(&pending, cancel).await;
    assert!(
        matches!(result, ModelOutcome::Failed(ref error) if error.message == "Request timed out")
    );
    assert_eq!(start.elapsed(), Duration::from_secs(15));
    assert_eq!(model.stats().errors, 1);
}

#[test]
fn coalesces_pending_work_while_a_request_is_running() {
    let mut queue = ModelQueue::default();
    queue.submit(work("first"));
    assert_eq!(queue.take_ready(500), Some(work("first")));
    queue.submit(work("discarded"));
    queue.submit(work("latest"));
    assert_eq!(queue.take_ready(5_000), None);
    queue.complete();
    assert_eq!(queue.next_due_ms(), Some(2_000));
    assert_eq!(queue.take_ready(1_999), None);
    assert_eq!(queue.take_ready(2_000), Some(work("latest")));
}

#[tokio::test]
#[ignore = "requires the Electron checkout and its node_modules"]
async fn replays_recorded_responses_and_matches_managed_gateway_bodies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let electron = std::env::var_os("KAIRO_ELECTRON_DIR")
        .map_or_else(|| root.join("../kairo-presenter"), std::path::PathBuf::from)
        .canonicalize()
        .unwrap();
    let raw = json!([
        {"book": "John", "chapter": 3, "verseStart": 16, "confidence": 1.2,
            "detectionType": "explicit", "sourceText": "John 3:16 in the NLT"},
        {"book": "Romans", "chapter": 8, "verseStart": 99, "confidence": 0.8,
            "detectionType": "explicit", "sourceText": "Romans 8:99"},
        {"book": "James", "chapter": 1, "verseStart": 4, "confidence": 0.6,
            "detectionType": "paraphrase", "sourceText": "faith produces patience"},
    ]);
    let wrapped = format!("\x60\x60\x60json\n{raw}\n\x60\x60\x60");
    let text = "The Bible says this morning there is hope.";
    let variants = [
        (ModelProvider::Deepseek, None, deepseek(&wrapped)),
        (
            ModelProvider::Anthropic,
            Some("custom-haiku".into()),
            anthropic(&wrapped),
        ),
        (ModelProvider::Deepseek, Some(String::new()), deepseek("[]")),
    ];
    let input: Vec<_> = variants
        .iter()
        .map(|(provider, model, response)| {
            let provider = match provider {
                ModelProvider::Deepseek => "deepseek",
                ModelProvider::Anthropic => "anthropic",
            };
            json!({"provider": provider, "model": model, "text": text, "response": response})
        })
        .collect();
    let mut child = Command::new(electron.join("node_modules/.bin/electron"))
        .args(["--import", "tsx", "--require", "./scripts/test-setup.cjs"])
        .arg(root.join("tools/ts-oracle/modules/detector-model/child.mts"))
        .current_dir(electron)
        .env("ELECTRON_RUN_AS_NODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&input).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected: Vec<Value> = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(expected.len(), variants.len());

    for ((provider, model, response), expected) in variants.into_iter().zip(expected) {
        let config = ModelConfig {
            provider,
            model,
            ..ModelConfig::default()
        };
        assert_eq!(json!([model_request(&config, text)]), expected["requests"]);
        let transport = Transport::default();
        transport.replies.lock().unwrap().push_back(Ok(response));
        let mut model = ManagedModelDetector::new(&transport, config);
        let (_sender, cancel) = watch::channel(false);
        let pending = work(text);
        let ModelOutcome::Parsed(detections) = model.run(&pending, cancel).await else {
            panic!("expected parsed response");
        };
        let mut local = LiveCitationDetector::default();
        let emissions = local.publish_model_result(&pending, detections, &[], 600);
        let detections: Vec<_> = emissions
            .iter()
            .map(|emission| {
                let r = &emission.reference;
                let detection_type = match emission.detection_type {
                    DetectionType::Explicit => "explicit",
                    DetectionType::Partial => "partial",
                    DetectionType::Quote => "quote",
                    DetectionType::Paraphrase => "paraphrase",
                };
                json!([
                    r.book,
                    r.chapter as u32,
                    r.verse_start as u32,
                    r.verse_end.map(|v| v as u32),
                    detection_type,
                    emission.confidence,
                    r.source_text,
                    emission.translations
                ])
            })
            .collect();
        // JavaScript serializes integral confidence values without a fraction.
        assert_eq!(
            json!(detections).to_string().replace(",1.0,", ",1,"),
            expected["detections"].to_string()
        );
    }
}

#[tokio::test(start_paused = true)]
async fn reports_gateway_errors_with_the_original_reason_and_counts_attempts() {
    for (status, message, attempts) in [
        (Some(401), "This installation is not activated", 1),
        (Some(402), "This month's minutes are used up", 1),
        (
            None,
            "Could not reach Kairo. Check your internet connection.",
            3,
        ),
        (Some(429), "Rate limit exceeded", 12),
    ] {
        let transport = Transport::default();
        let error = ModelError::gateway(status, message.into());
        transport
            .replies
            .lock()
            .unwrap()
            .extend(std::iter::repeat_n(Err(error.clone()), attempts));
        let mut model = ManagedModelDetector::new(&transport, ModelConfig::default());
        let (_sender, cancel) = watch::channel(false);
        assert_eq!(
            model.run(&work("The Bible says"), cancel).await,
            ModelOutcome::Failed(error)
        );
        assert_eq!(transport.requests.lock().unwrap().len(), attempts);
        assert_eq!(model.stats().errors, 1);
        assert_eq!(model.stats().total_calls, 0);
    }
}

#[tokio::test]
async fn stale_model_results_share_dedup_and_require_local_quote_evidence() {
    use kairo_scripture::service::QuoteCandidate;

    let result = json!([
        {"book": "John", "chapter": 3, "verseStart": 16, "confidence": 0.9,
            "detectionType": "explicit", "sourceText": "John 3:16"},
        {"book": "John", "chapter": 3, "verseStart": 17, "confidence": 0.8,
            "detectionType": "partial", "sourceText": "John 3:17"},
        {"book": "Romans", "chapter": 8, "verseStart": 28, "confidence": 0.9,
            "detectionType": "quote", "sourceText": "all things work together for good"},
        {"book": "Matthew", "chapter": 5, "verseStart": 3, "confidence": 0.9,
            "detectionType": "quote", "sourceText": "blessed are the poor in spirit"},
    ]);
    let transport = Transport::default();
    transport
        .replies
        .lock()
        .unwrap()
        .push_back(Ok(deepseek(&result.to_string())));
    let mut model = ManagedModelDetector::new(&transport, ModelConfig::default());
    let mut local = LiveCitationDetector::default();
    let work = work("The Bible says");
    let first = local.process(TranscriptEvent {
        at_ms: 600,
        kind: TranscriptKind::Final,
        text: "John 3:16",
        audio_epoch: None,
    });
    local.release(&first[0].reference);
    let (_sender, cancel) = watch::channel(false);
    let ModelOutcome::Parsed(parsed) = model.run(&work, cancel).await else {
        panic!("expected parsed response");
    };
    let evidence = [QuoteCandidate {
        book: "Romans".into(),
        chapter: 8.0,
        verse: 28.0,
        text: "all things work together for good".into(),
    }];
    let output = local.publish_model_result(&work, parsed, &evidence, 700);
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].reference.book, "Romans");
    assert!(output[0].superseded);
    assert_eq!(output[0].received_at_ms, 500);
    assert_eq!(output[0].emitted_at_ms, 700);
}

#[test]
fn offline_fallback_uses_regex_without_starting_or_throttling_model_work() {
    let mut queue = ModelQueue::default();
    queue.submit(work("In Romans chapter 8"));
    let work = queue.take_fallback(500).unwrap();
    let mut local = LiveCitationDetector::default();
    let output = local.run_regex_fallback(&work, 500);
    assert_eq!(output[0].reference.book, "Romans");
    assert_eq!(output[0].reference.verse_start, 1.0);
    queue.submit(work);
    assert_eq!(queue.next_due_ms(), Some(0));
}
