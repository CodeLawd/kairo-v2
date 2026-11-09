//! Health of one fan-out push, from `src/lib/output-health.ts`.

use crate::overlay::OverlayDispatchResult;
use crate::status::HealthStatus;
use serde::Serialize;

/// `HealthLine`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthLine {
    pub status: HealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// The `dispatchHealth` result: `{ output: HealthLine }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DispatchHealth {
    pub output: HealthLine,
}

/// `dispatchHealth`: error when nothing is enabled or everything failed,
/// degraded when some failed, ok otherwise.
pub fn dispatch_health(results: &[OverlayDispatchResult]) -> DispatchHealth {
    DispatchHealth {
        output: output_line(results),
    }
}

fn output_line(results: &[OverlayDispatchResult]) -> HealthLine {
    let failed: Vec<&OverlayDispatchResult> = results.iter().filter(|r| !r.ok).collect();
    let describe = || {
        let parts: Vec<String> = failed
            .iter()
            .map(|f| format!("{} ({})", f.name, f.reason.as_deref().unwrap_or("unknown")))
            .collect();
        format!("Output failed: {}", parts.join("; "))
    };
    if results.is_empty() {
        HealthLine {
            status: HealthStatus::Error,
            last_error: Some("No outputs are enabled".into()),
        }
    } else if failed.len() == results.len() {
        HealthLine {
            status: HealthStatus::Error,
            last_error: Some(describe()),
        }
    } else if !failed.is_empty() {
        HealthLine {
            status: HealthStatus::Degraded,
            last_error: Some(describe()),
        }
    } else {
        HealthLine {
            status: HealthStatus::Ok,
            last_error: None,
        }
    }
}
