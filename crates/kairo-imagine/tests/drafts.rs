use std::sync::Mutex;

use kairo_imagine::openai::BoxFuture;
use kairo_imagine::work::Look;
use kairo_imagine::{
    ImageModel, ImageRequest, ImageTransport, ImagineError, ImagineStore, Quality, work,
};

/// Answers from a script, recording what it was asked.
struct Scripted {
    answers: Mutex<Vec<Result<Vec<u8>, ImagineError>>>,
    seen: Mutex<Vec<ImageRequest>>,
}

impl Scripted {
    fn new(answers: Vec<Result<Vec<u8>, ImagineError>>) -> Self {
        Self {
            answers: Mutex::new(answers),
            seen: Mutex::default(),
        }
    }
}

impl ImageTransport for Scripted {
    fn image(&self, request: ImageRequest) -> BoxFuture<'_, Result<Vec<u8>, ImagineError>> {
        self.seen.lock().unwrap().push(request);
        let answer = self.answers.lock().unwrap().remove(0);
        Box::pin(async move { answer })
    }
}

fn open(dir: &tempfile::TempDir) -> Mutex<ImagineStore> {
    Mutex::new(ImagineStore::open(dir.path()).unwrap())
}

#[tokio::test]
async fn refinements_add_versions_from_the_chosen_reference() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let original = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
    let transport = Scripted::new(vec![Ok(original.clone()), Ok(vec![9]), Ok(vec![8])]);

    let project = work::generate(
        &store,
        &transport,
        "Sunrise",
        "A sunrise over still water, wide and calm, no text",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Medium,
        },
    )
    .await
    .unwrap();
    let v1 = project.selected.clone();
    // The operator's words and the enhanced prompt are both kept.
    assert_eq!(project.current().instruction, "Sunrise");
    assert_eq!(
        project.current().prompt.as_deref(),
        Some("A sunrise over still water, wide and calm, no text")
    );
    // Bytes are kept exactly as returned.
    {
        let s = store.lock().unwrap();
        assert_eq!(
            std::fs::read(s.file_path(&project.current().file)).unwrap(),
            original
        );
    }

    let p = work::refine(
        &store,
        &transport,
        &project.id,
        &v1,
        "warmer",
        "warmer",
        None,
        Look {
            model: ImageModel::Sunburst,
            quality: Quality::High,
        },
    )
    .await
    .unwrap();
    let v2 = p.selected.clone();
    assert_eq!(p.versions.len(), 2);
    assert_eq!(p.current().parent.as_deref(), Some(v1.as_str()));

    // Refine from the original: the edit's reference is v1, not v2.
    let p = work::refine(
        &store,
        &transport,
        &project.id,
        &v1,
        "add clouds",
        "add clouds",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Low,
        },
    )
    .await
    .unwrap();
    assert_eq!(p.versions.len(), 3);
    assert_eq!(p.current().parent.as_deref(), Some(v1.as_str()));
    let seen = transport.seen.lock().unwrap();
    assert_eq!(seen[0].reference, None);
    // The chosen model reaches both generation and refinement, and is recorded.
    assert_eq!(seen[0].model, ImageModel::Flare);
    assert_eq!(seen[1].model, ImageModel::Sunburst);
    assert_eq!(p.versions[1].model, Some(ImageModel::Sunburst));
    assert_eq!(p.versions[0].model, Some(ImageModel::Flare));
    assert_eq!(seen[1].reference.as_deref(), Some(original.as_slice()));
    assert_eq!(seen[2].reference.as_deref(), Some(original.as_slice()));
    drop(seen);

    // Restore v2 without losing anything.
    let s = store.lock().unwrap();
    let restored = s.select(&project.id, &v2).unwrap();
    assert_eq!(restored.selected, v2);
    assert_eq!(restored.versions.len(), 3);
}

#[tokio::test]
async fn a_failed_refinement_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let transport = Scripted::new(vec![Ok(vec![1]), Err(ImagineError::Refused)]);
    let project = work::generate(
        &store,
        &transport,
        "Cross",
        "Cross",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Medium,
        },
    )
    .await
    .unwrap();
    let before = std::fs::read(
        dir.path()
            .join(format!("imagine/projects/{}.json", project.id)),
    )
    .unwrap();

    let err = work::refine(
        &store,
        &transport,
        &project.id,
        &project.selected,
        "x",
        "x",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Medium,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err, ImagineError::Refused);
    let after = std::fs::read(
        dir.path()
            .join(format!("imagine/projects/{}.json", project.id)),
    )
    .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        std::fs::read_dir(dir.path().join("imagine/files"))
            .unwrap()
            .count(),
        1
    );
}

#[tokio::test]
async fn a_cancelled_job_records_nothing_and_strays_are_swept() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    struct Never;
    impl ImageTransport for Never {
        fn image(&self, _: ImageRequest) -> BoxFuture<'_, Result<Vec<u8>, ImagineError>> {
            Box::pin(std::future::pending())
        }
    }
    let job = work::generate(
        &store,
        &Never,
        "x",
        "x",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Low,
        },
    );
    // Cancel = drop the future before it finishes.
    let _ = tokio::time::timeout(std::time::Duration::from_millis(10), job).await;
    assert!(store.lock().unwrap().list().unwrap().is_empty());

    // A stray file (job stopped after writing its image) goes on reopen.
    std::fs::write(dir.path().join("imagine/files/stray.png"), b"x").unwrap();
    drop(store);
    let _ = open(&dir);
    assert_eq!(
        std::fs::read_dir(dir.path().join("imagine/files"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn saves_are_recorded_and_delete_frees_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let transport = Scripted::new(vec![Ok(vec![1])]);
    let project = work::generate(
        &store,
        &transport,
        "x",
        "x",
        None,
        Look {
            model: ImageModel::Flare,
            quality: Quality::Low,
        },
    )
    .await
    .unwrap();
    let s = store.lock().unwrap();
    let p = s.mark_saved(&project.id, &project.selected).unwrap();
    assert_eq!(p.saved, vec![project.selected.clone()]);
    assert_eq!(s.list().unwrap().len(), 1);
    s.delete(&project.id).unwrap();
    assert!(s.list().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(dir.path().join("imagine/files"))
            .unwrap()
            .count(),
        0
    );
    assert!(s.get("../x").is_err());
}

use kairo_imagine::{DeckPlan, DeckTheme, Layout, SlidePlan};

/// A real, decodable PNG, as the image model would return.
fn png(shade: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(32, 18, image::Rgba([shade, 60, 90, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn deck_plan(slides: &[(Layout, &str, bool)]) -> DeckPlan {
    DeckPlan {
        title: "AI for Church Media".into(),
        rhythm: Vec::new(),
        theme: DeckTheme::default(),
        background_prompt: "soft hills".into(),
        slides: slides
            .iter()
            .map(|(layout, title, keep)| SlidePlan {
                role: String::new(),
                layout: *layout,
                plan: String::new(),
                side: Default::default(),
                title: title.to_string(),
                eyebrow: String::new(),
                lead: String::new(),
                points: ["One point", "Another point"]
                    .map(|h| kairo_imagine::Point {
                        heading: h.into(),
                        ..Default::default()
                    })
                    .to_vec(),
                callout: String::new(),
                shapes: Vec::new(),
                notes: "Say hello.".into(),
                art_prompt: (*layout == Layout::ImageText).then(|| format!("art for {title}")),
                keep: *keep,
            })
            .collect(),
    }
}

const LOOK: Look = Look {
    model: ImageModel::Flare,
    quality: Quality::Medium,
};

#[tokio::test]
async fn a_deck_is_written_then_drawn_and_edits_keep_what_stays() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let transport = Scripted::new(vec![Ok(png(1)), Ok(png(2)), Ok(png(3))]);
    let seen_progress = std::sync::Mutex::new(Vec::new());
    let progress = |step| {
        if let work::Step::Painting { done, total } = step {
            seen_progress.lock().unwrap().push((done, total));
        }
    };
    let plan = deck_plan(&[
        (Layout::Title, "AI for Church Media", false),
        (Layout::ImageText, "Captions", false),
        (Layout::Bullets, "Planning", false),
    ]);
    let deck = work::deck(
        &store,
        &transport,
        None,
        "3 slides",
        Some("Here you go".into()),
        &plan,
        LOOK,
        &progress,
    )
    .await
    .unwrap();
    assert_eq!(deck.kind, kairo_imagine::Kind::Deck);
    assert_eq!(
        deck.title, "AI for Church Media",
        "the deck is named by the assistant"
    );
    let v1 = deck.current().clone();
    assert_eq!(v1.slides.len(), 3);
    assert!(v1.theme.is_some() && v1.background.is_some());
    // One picture (the text-and-picture slide) and one background: 2 images, not 3.
    assert_eq!(seen_progress.lock().unwrap().last(), Some(&(2, 2)));
    assert!(v1.slides[1].art.is_some() && v1.slides[0].art.is_none());
    assert_eq!(v1.slides[2].points[1].heading, "Another point");
    // Pictures never ask for text; slides are drawn by Kairo at 1920x1080.
    assert!(
        transport
            .seen
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.prompt.contains("No text"))
    );
    {
        let s = store.lock().unwrap();
        let img = image::load_from_memory(&s.read_file(&v1.slides[0].file).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (1920, 1080));
    }

    // Keep 1 and 2, rewrite 3, add a 4th with a new picture.
    let plan = deck_plan(&[
        (Layout::Title, "AI for Church Media", true),
        (Layout::ImageText, "Captions", true),
        (Layout::Bullets, "Planning ahead", false),
        (Layout::ImageText, "Streaming", false),
    ]);
    let p = work::deck(
        &store,
        &transport,
        Some((&deck.id, &v1.id)),
        "change slide 3",
        None,
        &plan,
        LOOK,
        &|_| {},
    )
    .await
    .unwrap();
    let v2 = p.current();
    assert_eq!(v2.slides.len(), 4);
    assert_eq!(
        v2.slides[1].art, v1.slides[1].art,
        "a kept picture is reused"
    );
    assert_eq!(
        v2.background, v1.background,
        "the background is painted once per deck"
    );
    assert_eq!(v2.slides[2].title, "Planning ahead");
    assert!(v2.slides[3].art.is_some());
    assert_eq!(
        transport.seen.lock().unwrap().len(),
        3,
        "only the new picture was painted"
    );
}

#[tokio::test]
async fn a_deck_that_fails_part_way_records_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir);
    let transport = Scripted::new(vec![Ok(png(1)), Err(ImagineError::Refused)]);
    let plan = deck_plan(&[
        (Layout::ImageText, "A", false),
        (Layout::ImageText, "B", false),
    ]);
    let err = work::deck(
        &store,
        &transport,
        None,
        "2 slides",
        None,
        &plan,
        LOOK,
        &|_| {},
    )
    .await
    .unwrap_err();
    assert_eq!(err, ImagineError::Refused);
    assert!(store.lock().unwrap().list().unwrap().is_empty());
    assert_eq!(
        std::fs::read_dir(dir.path().join("imagine/files"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn the_plan_streams_thoughts_and_falls_back_without_them() {
    use serde_json::{Value, json};
    struct Planner {
        stream_ok: bool,
    }
    fn answer() -> Value {
        let plan = json!({"reply": "One image coming up.", "action": "image", "image_prompt": "a dove", "deck": null});
        json!({"output": [{"type": "message", "content": [{"type": "output_text", "text": plan.to_string()}]}]})
    }
    impl ImageTransport for Planner {
        fn image(&self, _: ImageRequest) -> BoxFuture<'_, Result<Vec<u8>, ImagineError>> {
            Box::pin(async { Ok(vec![1]) })
        }
        fn respond(&self, body: Value) -> BoxFuture<'_, Result<Value, ImagineError>> {
            assert!(
                body.get("reasoning").is_none(),
                "the fallback asks without summaries"
            );
            Box::pin(async { Ok(answer()) })
        }
        fn respond_streaming<'a>(
            &'a self,
            body: Value,
            thought: &'a (dyn Fn(&str) + Send + Sync),
        ) -> BoxFuture<'a, Result<Value, ImagineError>> {
            assert_eq!(body["reasoning"]["summary"], "auto");
            let ok = self.stream_ok;
            Box::pin(async move {
                if ok {
                    thought("Picking ");
                    thought("a dove");
                    Ok(answer())
                } else {
                    Err(ImagineError::Api("summaries need verification".into()))
                }
            })
        }
    }
    let heard = std::sync::Mutex::new(String::new());
    let listen = |s: &str| heard.lock().unwrap().push_str(s);
    let plan = work::plan(&Planner { stream_ok: true }, None, "a dove", &listen)
        .await
        .unwrap();
    assert_eq!(plan.image_prompt.as_deref(), Some("a dove"));
    assert_eq!(*heard.lock().unwrap(), "Picking a dove");

    let plan = work::plan(&Planner { stream_ok: false }, None, "a dove", &|_| {})
        .await
        .unwrap();
    assert_eq!(plan.reply, "One image coming up.");
}
