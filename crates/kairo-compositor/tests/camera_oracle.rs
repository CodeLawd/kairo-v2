//! `videoLayout`, `applyCameraLayout` and `applyCameraNodeStyle`, run from
//! Electron's own `overlay.html` in Node, against the Rust port.
//!
//! The cases and Electron's answers are cached in
//! `tests/oracle/camera-layout.json`, so CI needs no Electron checkout.
//! Where the checkout and `node` are present the oracle also runs live and
//! must agree with the cache (drift shows up as a failure). Regenerate with
//! `KAIRO_REFRESH_ORACLE=1`.

use kairo_compositor::*;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cache_path() -> PathBuf {
    manifest().join("tests/oracle/camera-layout.json")
}

fn overlay_html() -> PathBuf {
    manifest().join("../../../kairo-presenter/src/main/services/output/overlay.html")
}

/// The source of `function name(...) { ... }` in `html`, by brace matching.
fn function_source(html: &str, name: &str) -> String {
    let start = html
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("{name} not found in overlay.html"));
    let open = start + html[start..].find('{').unwrap();
    let mut depth = 0;
    for (offset, c) in html[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return html[start..=open + offset].to_owned();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced {name}");
}

/// xorshift64*, so the corpus is the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a>(&mut self, items: &'a [Value]) -> &'a Value {
        &items[(self.next() % items.len() as u64) as usize]
    }
}

fn corpus() -> Vec<Value> {
    let numbers: Vec<Value> = vec![
        Value::Null,
        json!(0),
        json!(-5),
        json!(5),
        json!(12.5),
        json!(30),
        json!(45),
        json!(70),
        json!(95),
        json!(150),
        json!("40"),
        json!(" 25 "),
        json!("12px"),
        json!(""),
        json!(true),
        json!(false),
        json!([]),
        json!(["33"]),
        json!({}),
    ];
    let layouts = vec![
        json!("full"),
        json!("pip"),
        json!("split"),
        json!("PIP"),
        json!(""),
        Value::Null,
        json!(3),
    ];
    let corners = vec![
        json!("top-left"),
        json!("top-right"),
        json!("bottom-left"),
        json!("bottom-right"),
        json!(""),
        Value::Null,
        json!(0),
        json!(["right"]),
        json!("middle"),
    ];
    let sides = vec![
        json!("left"),
        json!("right"),
        json!("LEFT"),
        Value::Null,
        json!(1),
    ];
    let mut rng = Rng(0x5EED_CAFE_F00D_BEEF);
    let mut cases = vec![
        Value::Null,
        json!({}),
        json!(5),
        json!("pip"),
        json!({"layout": "pip", "box": {"xPct": 0, "yPct": 0, "widthPct": 25, "heightPct": 25}}),
        json!({"layout": "pip", "box": "nope"}),
        json!({"layout": "pip", "corner": 5}),
        json!({"layout": "split", "splitPct": 0, "side": "left"}),
    ];
    for _ in 0..600 {
        let mut case = serde_json::Map::new();
        let mut maybe = |rng: &mut Rng, key: &str, from: &[Value]| {
            if !rng.next().is_multiple_of(4) {
                case.insert(key.into(), rng.pick(from).clone());
            }
        };
        maybe(&mut rng, "layout", &layouts);
        maybe(&mut rng, "corner", &corners);
        maybe(&mut rng, "sizePct", &numbers);
        maybe(&mut rng, "marginPct", &numbers);
        maybe(&mut rng, "radiusPx", &numbers);
        maybe(&mut rng, "side", &sides);
        maybe(&mut rng, "splitPct", &numbers);
        if !rng.next().is_multiple_of(3) {
            let mut b = serde_json::Map::new();
            for key in ["xPct", "yPct", "widthPct", "heightPct"] {
                if !rng.next().is_multiple_of(4) {
                    b.insert(key.into(), rng.pick(&numbers).clone());
                }
            }
            case.insert("box".into(), Value::Object(b));
        }
        cases.push(Value::Object(case));
    }
    cases
}

/// Electron's answers: for each case, the first `videoLayout` pass (or its
/// error) and the styles `applyCameraLayout` leaves with a camera live, at
/// frame heights 1080 and 1440.
fn run_oracle(cases: &[Value]) -> Option<Vec<Value>> {
    let html = std::fs::read_to_string(overlay_html()).ok()?;
    let script = format!(
        r#"
var FRAME_W = 1920, FRAME_H = 1080;
var config = {{}}, cameraNode = null;
function el() {{ return {{ style: {{}}, classList: {{ on: {{}}, toggle: function (k, v) {{ this.on[k] = !!v; }} }} }}; }}
var nodes = {{}};
var document = {{ getElementById: function (id) {{ return nodes[id] || (nodes[id] = el()); }} }};
{video_layout}
{apply}
{node_style}
var cases = JSON.parse(require('fs').readFileSync(0, 'utf8'));
var out = cases.map(function (raw) {{
  var first;
  try {{ first = videoLayout(raw); }} catch (e) {{ return {{ error: String(e && e.name) }}; }}
  var result = {{ first: first, frames: {{}} }};
  [1080, 1440].forEach(function (h) {{
    FRAME_H = h; nodes = {{}};
    config = {{ video: first }};
    cameraNode = el();
    applyCameraLayout();
    var bg = document.getElementById('pa-content-bg');
    result.frames[h] = {{
      pip: !!document.getElementById('pa-program').classList.on['camera-pip'],
      backdrop: document.getElementById('pa-split-backdrop').style.display,
      transform: bg.style.transform,
      node: cameraNode.style,
    }};
  }});
  return result;
}});
process.stdout.write(JSON.stringify(out));
"#,
        video_layout = function_source(&html, "videoLayout"),
        apply = function_source(&html, "applyCameraLayout"),
        node_style = function_source(&html, "applyCameraNodeStyle"),
    );
    let mut child = Command::new("node")
        .arg("-e")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .ok()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(cases).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().ok()?;
    assert!(output.status.success(), "oracle failed");
    serde_json::from_slice(&output.stdout).ok()
}

/// A CSS length in frame pixels: `12px`, `30%` of `whole`, or `0`.
fn length(value: &Value, whole: f64) -> f64 {
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("css length {value}"));
    if let Some(px) = text.strip_suffix("px") {
        px.parse().unwrap()
    } else if let Some(pct) = text.strip_suffix('%') {
        pct.parse::<f64>().unwrap() / 100.0 * whole
    } else {
        text.parse().unwrap()
    }
}

/// `translate(Xpx,Ypx) scale(S)` or empty.
fn transform(text: &str) -> Transform {
    if text.is_empty() {
        return Transform::IDENTITY;
    }
    let numbers: Vec<f32> = text
        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
        .filter(|s| !s.is_empty() && *s != "e")
        .map(|s| s.parse().unwrap())
        .collect();
    Transform {
        x: numbers[0],
        y: numbers[1],
        scale: numbers[2],
    }
}

fn near(a: f32, b: f64, what: &str) {
    assert!((f64::from(a) - b).abs() < 1e-3, "{what}: {a} vs {b}");
}

#[test]
fn camera_layout_matches_electron() {
    let refresh = std::env::var_os("KAIRO_REFRESH_ORACLE").is_some();
    let cases = corpus();
    let cached: Option<Value> = std::fs::read(cache_path())
        .ok()
        .map(|b| serde_json::from_slice(&b).unwrap());
    let live = run_oracle(&cases);
    let answers: Vec<Value> = match (&live, &cached) {
        (Some(live), _) if refresh => {
            std::fs::create_dir_all(cache_path().parent().unwrap()).unwrap();
            let file = json!({ "source": "src/main/services/output/overlay.html", "cases": cases, "answers": live });
            std::fs::write(cache_path(), serde_json::to_string(&file).unwrap()).unwrap();
            live.clone()
        }
        (Some(live), Some(cached)) => {
            assert_eq!(
                &cached["cases"],
                &json!(cases),
                "corpus changed; refresh the cache"
            );
            assert_eq!(
                &cached["answers"],
                &json!(live),
                "Electron's camera layout changed since the cache; port it, then refresh"
            );
            live.clone()
        }
        (None, Some(cached)) => {
            eprintln!("Electron or node unavailable: using the cached oracle");
            serde_json::from_value(cached["answers"].clone()).unwrap()
        }
        (_, None) => panic!(
            "no cache at {}; run with KAIRO_REFRESH_ORACLE=1",
            cache_path().display()
        ),
    };
    assert_eq!(answers.len(), cases.len());
    let mut errors = 0;
    for (case, answer) in cases.iter().zip(&answers) {
        let ours = video_layout(Some(case));
        if answer.get("error").is_some() {
            assert_eq!(ours, Err(camera::LayoutError), "{case}");
            errors += 1;
            continue;
        }
        let first = ours.unwrap_or_else(|_| panic!("{case} threw only here"));
        let theirs = &answer["first"];
        assert_eq!(first.to_value()["layout"], theirs["layout"], "{case}");
        for key in ["xPct", "yPct", "widthPct", "heightPct"] {
            assert_eq!(
                first.to_value()["box"][key].as_f64(),
                theirs["box"][key].as_f64(),
                "{case} {key}"
            );
        }
        for key in ["radiusPx", "splitPct"] {
            assert_eq!(
                first.to_value()[key].as_f64(),
                theirs[key].as_f64(),
                "{case} {key}"
            );
        }
        assert_eq!(first.to_value()["side"], theirs["side"], "{case}");
        for height in [1080u32, 1440] {
            let frame = &answer["frames"][height.to_string()];
            let placed = place(&first, true, height);
            assert_eq!(
                placed.mode == Mode::Pip,
                frame["pip"] == json!(true),
                "{case}"
            );
            assert_eq!(
                placed.backdrop,
                frame["backdrop"] == json!("block"),
                "{case}"
            );
            let t = transform(frame["transform"].as_str().unwrap());
            near(placed.content.x, f64::from(t.x), "translate x");
            near(placed.content.y, f64::from(t.y), "translate y");
            near(placed.content.scale, f64::from(t.scale), "scale");
            let node = &frame["node"];
            let (w, h) = (1920.0, f64::from(height));
            near(placed.camera.x, length(&node["left"], w), "left");
            near(placed.camera.y, length(&node["top"], h), "top");
            near(placed.camera.width, length(&node["width"], w), "width");
            near(placed.camera.height, length(&node["height"], h), "height");
            near(placed.radius, length(&node["borderRadius"], 0.0), "radius");
            assert_eq!(
                placed.shadow,
                node["boxShadow"] != json!("none"),
                "{case} shadow"
            );
        }
    }
    eprintln!("{} cases, {errors} threw in both", cases.len());
}
