//! Dev-only comparison of Rust ports against the original TypeScript.
//!
//! Use this crate only from `[dev-dependencies]`. Node never ships.
//!
//! # Comparison rules
//!
//! - Object key order is ignored.
//! - Numbers compare exactly by `f64` value, unless a test states a tolerance.
//!   `-0` equals `0`, as `JSON.stringify` makes them identical.
//! - `NaN` and infinities are tagged. Use [`number`] for Rust floats.
//! - An `undefined` object property equals an absent key, matching
//!   `#[serde(skip_serializing_if = "Option::is_none")]`. It never equals `null`.
//! - An `undefined` return value or array element needs [`undefined`].
//! - A TypeScript throw must be matched by [`thrown`]. Messages are not compared.
//!
//! # Modes, from `KAIRO_ORACLE_MODE`
//!
//! - `cache` (default): cached results only, so CI needs no Node.
//! - `live`: call the TypeScript, write nothing.
//! - `refresh`: call the TypeScript and rewrite the cache.
//!
//! Refresh everything with `cargo run -p ts-oracle -- --regenerate`.

use proptest::{
    strategy::{Strategy, ValueTree},
    test_runner::{Config as RunnerConfig, RngAlgorithm, TestRng, TestRunner},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, OnceLock},
};

const REFRESH: &str = "cargo run -p ts-oracle -- --regenerate";

/// The TypeScript `undefined`.
pub fn undefined() -> Value {
    json!({"$oracle": "undefined"})
}

/// A Rust output standing for "the TypeScript threw".
pub fn thrown() -> Value {
    json!({"$oracle": "throw"})
}

/// A float that keeps `NaN` and infinities, which plain JSON cannot hold.
pub fn number(value: f64) -> Value {
    if value.is_finite() {
        json!(value)
    } else {
        let text = if value.is_nan() {
            "NaN"
        } else if value > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        };
        json!({"$oracle": "number", "value": text})
    }
}

fn is_undefined(value: Option<&Value>) -> bool {
    value.is_none_or(|v| v.get("$oracle").and_then(Value::as_str) == Some("undefined"))
}

/// Differences between the oracle's value and the Rust value, as readable paths.
pub fn compare(expected: &Value, actual: &Value) -> Vec<String> {
    let mut out = Vec::new();
    walk("$", expected, actual, 0.0, &mut out);
    out
}

/// As [`compare`], allowing numbers to differ by up to `tolerance`.
pub fn compare_with_tolerance(expected: &Value, actual: &Value, tolerance: f64) -> Vec<String> {
    let mut out = Vec::new();
    walk("$", expected, actual, tolerance, &mut out);
    out
}

fn walk(path: &str, expected: &Value, actual: &Value, tolerance: f64, out: &mut Vec<String>) {
    let tag = |v: &Value| v.get("$oracle").and_then(Value::as_str).map(str::to_owned);
    if tag(expected).as_deref() == Some("throw") || tag(actual).as_deref() == Some("throw") {
        if tag(expected) != tag(actual) {
            out.push(format!("{path}: oracle {expected}, rust {actual}"));
        }
        return;
    }
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
            );
            if (a - b).abs() > tolerance {
                out.push(format!("{path}: oracle {a}, rust {b}"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
                let (x, y) = (a.get(key), b.get(key));
                let child = format!("{path}.{key}");
                match (x, y) {
                    (Some(x), Some(y)) => walk(&child, x, y, tolerance, out),
                    _ if is_undefined(x) && is_undefined(y) => {}
                    _ => out.push(format!("{child}: oracle {x:?}, rust {y:?}")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!(
                    "{path}: oracle length {}, rust length {}",
                    a.len(),
                    b.len()
                ));
            }
            for (index, (x, y)) in a.iter().zip(b).enumerate() {
                walk(&format!("{path}[{index}]"), x, y, tolerance, out);
            }
        }
        _ if expected == actual => {}
        _ => out.push(format!("{path}: oracle {expected}, rust {actual}")),
    }
}

/// `count` values from `strategy`. The same seed always gives the same values,
/// which is what lets cached results be found again.
pub fn generate<S: Strategy>(seed: u64, count: usize, strategy: S) -> Vec<S::Value> {
    let mut bytes = [0u8; 32];
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &bytes);
    let mut runner = TestRunner::new_with_rng(RunnerConfig::default(), rng);
    (0..count)
        .map(|_| {
            strategy
                .new_tree(&mut runner)
                .expect("generators in these tests never reject")
                .current()
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Cache,
    Live,
    Refresh,
}

struct Node {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Oracle {
    mode: Mode,
    cache: PathBuf,
    electron: PathBuf,
    lib: Option<PathBuf>,
    node: Option<Node>,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fnv(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

/// The mode named by `KAIRO_ORACLE_MODE`, `cache` when unset.
pub fn mode_from_env() -> Result<Mode, String> {
    parse_mode(std::env::var("KAIRO_ORACLE_MODE").ok().as_deref())
}

/// The Electron checkout: `KAIRO_ELECTRON_DIR`, else `../kairo-presenter`.
pub fn electron_dir() -> PathBuf {
    std::env::var_os("KAIRO_ELECTRON_DIR")
        .map_or_else(|| repo_root().join("../kairo-presenter"), PathBuf::from)
}

fn parse_mode(text: Option<&str>) -> Result<Mode, String> {
    match text {
        None | Some("cache") => Ok(Mode::Cache),
        Some("live") => Ok(Mode::Live),
        Some("refresh") => Ok(Mode::Refresh),
        Some(other) => Err(format!(
            "KAIRO_ORACLE_MODE must be cache, live or refresh, not {other}"
        )),
    }
}

impl Oracle {
    pub fn new(mode: Mode, cache: PathBuf, electron: PathBuf) -> Self {
        Self {
            mode,
            cache,
            electron,
            lib: None,
            node: None,
        }
    }

    /// Loads modules from `lib` instead of the Electron `src/lib`. The Electron
    /// checkout still supplies `tsx`. Used to test the runner itself.
    pub fn with_lib(mut self, lib: PathBuf) -> Self {
        self.lib = Some(lib);
        self
    }

    fn from_env() -> Self {
        let mode = mode_from_env().unwrap_or_else(|error| panic!("{error}"));
        Self::new(mode, repo_root().join("tests/oracle-cache"), electron_dir())
    }

    fn start(&mut self) -> Result<&mut Node, String> {
        if self.node.is_none() {
            let loader = self.electron.join("node_modules/tsx/dist/loader.mjs");
            if !loader.is_file() {
                return Err(format!(
                    "{} is missing. Set KAIRO_ELECTRON_DIR to an installed Electron checkout",
                    loader.display()
                ));
            }
            let mut command = Command::new("node");
            if let Some(lib) = &self.lib {
                command.env("KAIRO_ORACLE_LIB", lib);
            }
            let mut child = command
                .arg("--import")
                .arg(format!(
                    "file://{}",
                    loader.canonicalize().map_err(|e| e.to_string())?.display()
                ))
                .arg(repo_root().join("tools/ts-oracle/runner.mjs"))
                .env("KAIRO_ELECTRON_DIR", &self.electron)
                .current_dir(&self.electron)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .map_err(|e| format!("could not start node: {e}"))?;
            let stdin = child.stdin.take().ok_or("no stdin")?;
            let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
            self.node = Some(Node {
                child,
                stdin,
                stdout,
            });
        }
        Ok(self.node.as_mut().expect("just started"))
    }

    /// Runs one export over many argument lists through the live TypeScript.
    fn live(&mut self, name: &str, inputs: &[Value]) -> Result<Vec<Value>, String> {
        let node = self.start()?;
        let mut request = String::new();
        for args in inputs {
            // Node's readline also ends a line at U+2028 and U+2029, which
            // JSON may carry raw inside strings, so they travel escaped.
            let line = json!({"fn": name, "args": args})
                .to_string()
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029");
            request.push_str(&line);
            request.push('\n');
        }
        let Node { stdin, stdout, .. } = node;
        std::thread::scope(|scope| {
            let writer = scope.spawn(move || {
                stdin
                    .write_all(request.as_bytes())
                    .and_then(|()| stdin.flush())
            });
            let mut results = Vec::with_capacity(inputs.len());
            // Every reply is read even after a failure, so the next batch
            // never picks up this one's leftovers.
            let mut failure = None;
            let mut line = String::new();
            for _ in inputs {
                line.clear();
                let read = stdout.read_line(&mut line).map_err(|e| e.to_string())?;
                if read == 0 {
                    return Err("the oracle process exited early".to_owned());
                }
                let reply: Value =
                    serde_json::from_str(&line).map_err(|e| format!("bad oracle reply: {e}"))?;
                if reply["ok"] == true {
                    results.push(reply["value"].clone());
                } else if reply["error"]["kind"] == "throw" {
                    results.push(json!({"$oracle": "throw", "name": reply["error"]["name"], "message": reply["error"]["message"]}));
                } else if failure.is_none() {
                    failure = Some(format!(
                        "oracle failed for {name}: {}",
                        reply["error"]["message"]
                    ));
                }
            }
            writer
                .join()
                .map_err(|_| "writer panicked")?
                .map_err(|e| e.to_string())?;
            failure.map_or(Ok(results), Err)
        })
    }

    /// A file name that is valid on every platform: `/` and `:` (from
    /// `oracle:` helpers) are not allowed on Windows.
    fn cache_file(&self, name: &str) -> PathBuf {
        let safe = name.replace('/', "__").replace(':', "--");
        self.cache.join(format!("{safe}.jsonl"))
    }

    fn read_cache(&self, name: &str) -> Result<BTreeMap<String, Value>, String> {
        let Ok(text) = fs::read_to_string(self.cache_file(name)) else {
            return Ok(BTreeMap::new());
        };
        let mut entries = BTreeMap::new();
        for line in text.lines().skip(1) {
            let row: Value = serde_json::from_str(line)
                .map_err(|e| format!("corrupt cache for {name}: {e}. Run {REFRESH}"))?;
            entries.insert(
                row["k"].as_str().unwrap_or_default().to_owned(),
                row["r"].clone(),
            );
        }
        Ok(entries)
    }

    fn write_cache(&self, name: &str, entries: &BTreeMap<String, Value>) -> Result<(), String> {
        let commit = |args: &[&str]| {
            Command::new("git")
                .arg("--no-optional-locks")
                .arg("-C")
                .arg(&self.electron)
                .args(args)
                .output()
                .map_err(|e| e.to_string())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        };
        let hash = commit(&["rev-parse", "--short=10", "HEAD"])?;
        if hash.is_empty() {
            return Err(format!(
                "{} is not a git checkout, so results cannot be stamped",
                self.electron.display()
            ));
        }
        let dirty = !commit(&["status", "--porcelain", "--", "src/lib"])?.is_empty();
        let mut text =
            json!({"fn": name, "electron_commit": hash, "src_lib_dirty": dirty}).to_string();
        text.push('\n');
        for (key, value) in entries {
            text.push_str(&json!({"k": key, "r": value}).to_string());
            text.push('\n');
        }
        fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        fs::write(self.cache_file(name), text).map_err(|e| e.to_string())
    }

    /// The oracle's answer for each argument list, in order.
    pub fn results(&mut self, name: &str, inputs: &[Value]) -> Result<Vec<Value>, String> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<_> = inputs.iter().map(|args| fnv(&args.to_string())).collect();
        match self.mode {
            Mode::Live => self.live(name, inputs),
            Mode::Refresh => {
                let results = self.live(name, inputs)?;
                let mut entries = self.read_cache(name)?;
                entries.extend(keys.into_iter().zip(results.iter().cloned()));
                self.write_cache(name, &entries)?;
                Ok(results)
            }
            Mode::Cache => {
                let entries = self.read_cache(name)?;
                keys.iter()
                    .map(|key| {
                        entries.get(key).cloned().ok_or_else(|| {
                            format!("no cached oracle result for {name}. Run {REFRESH}")
                        })
                    })
                    .collect()
            }
        }
    }
}

fn shared() -> &'static Mutex<Oracle> {
    static ORACLE: OnceLock<Mutex<Oracle>> = OnceLock::new();
    ORACLE.get_or_init(|| Mutex::new(Oracle::from_env()))
}

/// The oracle's answer for one call, from the cache or the live TypeScript
/// as the mode says. For tests that need the value itself, such as a default
/// to build a Rust value from. Panics with the refresh command on a miss.
pub fn oracle_value(name: &str, args: Value) -> Value {
    shared()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .results(name, &[args])
        .unwrap_or_else(|error| panic!("{error}"))
        .remove(0)
}

/// Compares many cases at once. Each case is `(arguments array, Rust output)`.
pub fn assert_all_match(name: &str, cases: &[(Value, Value)]) {
    let inputs: Vec<_> = cases.iter().map(|(args, _)| args.clone()).collect();
    let expected = shared()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .results(name, &inputs)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut failures = 0;
    let mut report = String::new();
    for ((args, actual), expected) in cases.iter().zip(&expected) {
        let differences = compare(expected, actual);
        if !differences.is_empty() {
            failures += 1;
            if failures <= 5 {
                report.push_str(&format!("\ninput {args}\n  {}", differences.join("\n  ")));
            }
        }
    }
    assert_eq!(
        failures,
        0,
        "{name}: {failures} of {} cases differ{report}",
        cases.len()
    );
}

/// Compares one call. `input` is the array of arguments.
pub fn assert_matches_oracle(name: &str, input: Value, rust_output: Value) {
    assert_all_match(name, &[(input, rust_output)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_order_and_integer_forms_do_not_matter() {
        assert!(compare(&json!({"a": 1, "b": [2.0]}), &json!({"b": [2], "a": 1.0})).is_empty());
    }

    #[test]
    fn numbers_are_exact_unless_a_tolerance_is_given() {
        assert_eq!(compare(&json!(0.3), &json!(0.1 + 0.2)).len(), 1);
        assert!(compare_with_tolerance(&json!(0.3), &json!(0.1 + 0.2), 1e-9).is_empty());
    }

    #[test]
    fn undefined_rules() {
        assert!(compare(&json!({"a": undefined()}), &json!({})).is_empty());
        assert_eq!(
            compare(&json!({"a": undefined()}), &json!({"a": null})).len(),
            1
        );
        assert_eq!(compare(&json!({"a": null}), &json!({})).len(), 1);
        assert_eq!(compare(&undefined(), &Value::Null).len(), 1);
        assert_eq!(compare(&json!([undefined()]), &json!([])).len(), 1);
    }

    #[test]
    fn throws_must_be_matched() {
        let error = json!({"$oracle": "throw", "name": "E", "message": "m"});
        assert!(compare(&error, &thrown()).is_empty());
        assert_eq!(compare(&error, &json!(1)).len(), 1);
        assert_eq!(compare(&json!(1), &thrown()).len(), 1);
    }

    #[test]
    fn non_finite_numbers_compare_by_tag() {
        assert!(compare(&number(f64::NAN), &number(f64::NAN)).is_empty());
        assert_eq!(
            compare(&number(f64::INFINITY), &number(f64::NEG_INFINITY)).len(),
            1
        );
        assert_eq!(compare(&number(f64::NAN), &json!(0)).len(), 1);
    }

    #[test]
    fn generation_is_seeded() {
        assert_eq!(generate(7, 20, 0u32..1000), generate(7, 20, 0u32..1000));
        assert_ne!(generate(7, 20, 0u32..1000), generate(8, 20, 0u32..1000));
    }

    #[test]
    fn cache_miss_and_corruption_are_errors_that_name_the_fix() {
        let dir = std::env::temp_dir().join(format!("kairo-oracle-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let mut oracle = Oracle::new(Mode::Cache, dir.clone(), dir.join("none"));
        assert!(
            oracle
                .results("a.b", &[json!([])])
                .unwrap_err()
                .contains(REFRESH)
        );
        fs::write(dir.join("a.b.jsonl"), "{}\nnot json\n").unwrap();
        assert!(
            oracle
                .cache_file("oracle:x/y.z")
                .ends_with("oracle--x__y.z.jsonl")
        );
        assert!(
            oracle
                .results("a.b", &[json!([])])
                .unwrap_err()
                .contains("corrupt")
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_electron_checkout_is_reported() {
        let mut oracle = Oracle::new(
            Mode::Live,
            PathBuf::from("/nonexistent"),
            PathBuf::from("/nonexistent"),
        );
        assert!(
            oracle
                .results("a.b", &[json!([])])
                .unwrap_err()
                .contains("KAIRO_ELECTRON_DIR")
        );
    }

    #[test]
    fn modes() {
        assert_eq!(parse_mode(None), Ok(Mode::Cache));
        assert_eq!(parse_mode(Some("refresh")), Ok(Mode::Refresh));
        assert!(parse_mode(Some("x")).is_err());
    }
}
