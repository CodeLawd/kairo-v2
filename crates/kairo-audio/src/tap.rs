//! The sender side of an NDI tap: takes the mixer's planar blocks off the
//! audio thread and hands each, checked, to the NDI sender
//! (`window.kairoProgram.sendAudio` then `handleNdiAudio`).

use crate::block::{AudioBlock, BLOCK_FRAMES};
use crate::mixer::{RATE, TapOutput};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct TapSender {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TapSender {
    /// One thread for every tap: `send(tap, block)` gets each block, in
    /// order per tap. With no tap on it sleeps; it never touches a tap's
    /// samples while that tap is off.
    pub fn start(
        mut outputs: Vec<TapOutput>,
        mut send: impl FnMut(usize, AudioBlock) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::Builder::new()
            .name("ndi audio".into())
            .spawn(move || {
                let len = BLOCK_FRAMES * 2;
                while !done.load(Ordering::Acquire) {
                    let mut sent = false;
                    let mut any_on = false;
                    for (tap, output) in outputs.iter_mut().enumerate() {
                        any_on |= output.switch.is_on();
                        if output.consumer.slots() < len {
                            continue;
                        }
                        let Ok(chunk) = output.consumer.read_chunk(len) else {
                            continue;
                        };
                        let (a, b) = chunk.as_slices();
                        let mut data = Vec::with_capacity(len);
                        data.extend_from_slice(a);
                        data.extend_from_slice(b);
                        chunk.commit_all();
                        sent = true;
                        let block = AudioBlock {
                            sample_rate: f64::from(RATE),
                            channels: 2,
                            samples: BLOCK_FRAMES as u32,
                            data,
                        };
                        match block.validate() {
                            Ok(()) => send(tap, block),
                            Err(error) => tracing::warn!(%error, "NDI audio block refused"),
                        }
                    }
                    if !sent {
                        // A block is 21 ms; off, there is nothing to look at.
                        let nap = if any_on { 5 } else { 50 };
                        std::thread::sleep(Duration::from_millis(nap));
                    }
                }
            })
            .map_err(|e| tracing::error!("cannot start the NDI audio thread: {e}"))
            .ok();
        Self { stop, thread }
    }
}

impl Drop for TapSender {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mixer::{SourceKind, mixer};
    use std::sync::Mutex;

    #[test]
    fn checked_blocks_reach_the_sender_by_tap() {
        let (mut mixer, mut control, outputs) = mixer(2);
        let got: Arc<Mutex<Vec<(usize, AudioBlock)>>> = Arc::default();
        let sink = got.clone();
        let _sender = TapSender::start(outputs, move |tap, block| {
            sink.lock().unwrap().push((tap, block))
        });
        let (_, mut source) = control.add(SourceKind::Video).unwrap();
        source.control.set_taps(0b10);
        source.push(&vec![0.5; 2048 * 2]);
        control.tap(1).unwrap().set_on(true);
        let mut out = vec![0.0; 1024 * 2];
        mixer.render(&mut out);
        mixer.render(&mut out);
        let end = std::time::Instant::now() + Duration::from_secs(2);
        while got.lock().unwrap().len() < 2 && std::time::Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
        }
        let got = got.lock().unwrap();
        assert_eq!(got.len(), 2);
        let (tap, block) = &got[1];
        assert_eq!(*tap, 1);
        assert_eq!(
            (block.sample_rate, block.channels, block.samples),
            (48_000.0, 2, 1024)
        );
        assert_eq!(block.validate(), Ok(()));
        assert!(block.data.iter().all(|v| *v == 0.5));
    }
}
