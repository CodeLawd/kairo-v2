//! Program fan-out. From `src/main/services/output/dispatch.ts`.
//!
//! Every rendered destination whose content filter allows the kind is pushed
//! at once, and one failure cannot block another. Only when none took it are
//! fallback destinations tried, one at a time in order, stopping at the first
//! that does.

use crate::voice::BoxFuture;
use kairo_domain::overlay::{OverlayContentKind, OverlayDispatchResult, OverlayOutput};
use kairo_domain::overlay_outputs::get_dispatch_plan;

/// `push` returns `None` when the output took the slide, else why not.
pub async fn dispatch_to_outputs<'a>(
    outputs: &'a [OverlayOutput],
    kind: OverlayContentKind,
    push: impl Fn(&'a OverlayOutput) -> BoxFuture<'a, Result<Option<String>, String>>,
) -> Vec<OverlayDispatchResult> {
    let plan = get_dispatch_plan(outputs, |output| shows(output, kind));
    let attempt = |output: &'a OverlayOutput| {
        let pushed = push(output);
        async move {
            let (ok, reason) = match pushed.await {
                Ok(None) => (true, None),
                Ok(Some(reason)) | Err(reason) => (false, Some(reason)),
            };
            OverlayDispatchResult {
                output_id: output.id.clone(),
                name: output.name.clone(),
                kind: output.kind,
                ok,
                reason,
            }
        }
    };
    let mut results = futures_join(plan.rendered.iter().map(|o| attempt(o))).await;
    if !results.iter().any(|r| r.ok) {
        for output in plan.fallbacks {
            let result = attempt(output).await;
            let ok = result.ok;
            results.push(result);
            if ok {
                break;
            }
        }
    }
    results
}

/// `output.show[kind]`.
pub fn shows(output: &OverlayOutput, kind: OverlayContentKind) -> bool {
    match kind {
        OverlayContentKind::Scripture => output.show.scripture,
        OverlayContentKind::Lyrics => output.show.lyrics,
    }
}

/// `Promise.all`: run together, results in input order.
async fn futures_join<F: std::future::Future>(futures: impl Iterator<Item = F>) -> Vec<F::Output> {
    let mut pinned: Vec<_> = futures.map(Box::pin).collect();
    let mut out: Vec<Option<F::Output>> = (0..pinned.len()).map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (i, f) in pinned.iter_mut().enumerate() {
            if out[i].is_none() {
                match f.as_mut().poll(cx) {
                    std::task::Poll::Ready(v) => out[i] = Some(v),
                    std::task::Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    out.into_iter()
        .map(|v| v.expect("joined future finished"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay::OverlayOutputKind;
    use kairo_domain::overlay_defaults::make_overlay_output;
    use std::sync::Mutex;

    fn output(id: &str, kind: OverlayOutputKind, fallback: bool, order: f64) -> OverlayOutput {
        let mut o = make_overlay_output(id, kind);
        o.enabled = true;
        o.fallback_only = fallback;
        o.order = order;
        o
    }

    #[tokio::test]
    async fn every_rendered_output_is_pushed_and_one_failure_does_not_block() {
        let outputs = vec![
            output("screen", OverlayOutputKind::Screen, false, 0.0),
            output("feed", OverlayOutputKind::Ndi, false, 1.0),
            output("spare", OverlayOutputKind::Ndi, true, 2.0),
        ];
        let tried = Mutex::new(Vec::new());
        let results = dispatch_to_outputs(&outputs, OverlayContentKind::Scripture, |o| {
            tried.lock().unwrap().push(o.id.clone());
            let id = o.id.clone();
            Box::pin(async move {
                if id == "screen" {
                    Err("window crashed".into())
                } else {
                    Ok(None)
                }
            })
        })
        .await;
        assert_eq!(*tried.lock().unwrap(), ["screen", "feed"]);
        assert_eq!(results[0].reason.as_deref(), Some("window crashed"));
        assert!(results[1].ok);
    }

    #[tokio::test]
    async fn fallbacks_run_in_order_until_one_takes_it() {
        let outputs = vec![
            output("screen", OverlayOutputKind::Screen, false, 0.0),
            output("late", OverlayOutputKind::Ndi, true, 9.0),
            output("early", OverlayOutputKind::Screen, true, 1.0),
            output("never", OverlayOutputKind::Ndi, true, 10.0),
        ];
        let results = dispatch_to_outputs(&outputs, OverlayContentKind::Lyrics, |o| {
            let id = o.id.clone();
            Box::pin(async move {
                Ok(match id.as_str() {
                    "late" => None,
                    _ => Some("Screen window is not available".to_string()),
                })
            })
        })
        .await;
        let ids: Vec<_> = results
            .iter()
            .map(|r| (r.output_id.as_str(), r.ok))
            .collect();
        assert_eq!(ids, [("screen", false), ("early", false), ("late", true)]);
    }

    #[tokio::test]
    async fn no_destinations_means_no_results() {
        let results = dispatch_to_outputs(&[], OverlayContentKind::Scripture, |_| {
            Box::pin(async { Ok(None) })
        })
        .await;
        assert!(results.is_empty());
    }
}
