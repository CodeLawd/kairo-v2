# Live web output verification

Build the engine and driver first:

```sh
cargo build -p kairo-engine --bin kairo-engine --example drive --example verify_web
```

Run the automatic native browser/GPU rig (macOS verified; Windows runtime deferred):

```sh
cargo run -p kairo-engine --example verify_web
```

This starts its own localhost fixture. It verifies that two GPU outputs share
one page and identical pixels, input/scroll, native camera/microphone denial,
a user-activated cross-origin iframe top-navigation attack, malformed IPC,
reload, teardown with an outstanding capture, URL adoption and return to the
original page, and replacement at the page limit. It fails rather than silently
skipping an unsupported desktop/browser/capture environment.

Run the fullscreen occlusion regression and capture measurement with:

```sh
cargo run -p kairo-engine --example verify_web -- --occluded --measure
```

This checks animation while the source is fully covered and records 300 capture
samples after 30 warm-up frames (p99 must remain below 50 ms). The macOS backend
uses a guarded private WebKit visibility control; rerun this check when qualifying
macOS updates. See [native capture notes](../../native-migration/notes/059-web-capture.md).

To see the actual engine output, start the manual fixture in one terminal:

```sh
python3 tools/web-layer-site/serve.py
```

Then run in another:

```sh
cargo run -p kairo-engine --example drive -- --web http://127.0.0.1:8059 --web-exercise --text '' --reference '' --secs 60
```

The driver clicks the counter, scrolls, types, attempts camera/microphone access,
attempts a popup/download, plays a short local tone, and follows a link. The
server prints observations. `--web-exercise` is for this fixture's coordinates;
omit it for other pages. Use `--full` for a full-display output. `--web URL`
accepts other HTTP(S) sites. The operator's Web workspace is prompt 089; this
is the engine verification driver.

The fixture's main document denies framing with X-Frame-Options/CSP. The attack
frame itself allows framing so its navigation restriction can actually be tested.
A capture-only run can save composed evidence using `KAIRO_WEB_EVIDENCE_DIR`,
which writes `web-output.png` every two seconds on a worker. The engine logs
snapshot request→CPU copy and CPU copy→GPU-complete display submission timings.
Those timings exclude capture sampling age and physical display scanout.

Page audio goes through the OS browser's ordinary output. Public web-view APIs
do not supply a PCM feed to the Kairo mixer or isolated NDI taps.
