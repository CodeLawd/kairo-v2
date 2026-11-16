//! Sequential setup effects. User actions are single flight; automatic reconciliation
//! is coalesced by the wizard, never queued as stale navigation commands.
use crate::{backend::OnboardingWrite, model::Account};
use gpui_kit::{AsyncApp, Entity};
use kairo_domain::{
    cloud::can_enter_app,
    settings::{OnboardingState, OnboardingStepId},
};
/// What a write to setup does, run in order.
#[derive(Clone, Debug)]
pub(crate) enum Job {
    Seed,
    Write(OnboardingWrite),
    /// Write the next step only when not signed in and ready.
    LeaveTour,
}

#[derive(Debug)]
pub(crate) enum WriteError {
    Stale,
    Backend(String),
}
fn current(
    account: &Entity<Account>,
    epoch: (u64, u64),
    cx: &mut AsyncApp,
) -> Result<(), WriteError> {
    if cx.update(|cx| account.read(cx).write_epoch()) == epoch {
        Ok(())
    } else {
        Err(WriteError::Stale)
    }
}
pub(crate) async fn run(
    account: &Entity<Account>,
    jobs: Vec<Job>,
    epoch: (u64, u64),
    cx: &mut AsyncApp,
) -> Result<Option<OnboardingState>, WriteError> {
    let mut state = None;
    for job in jobs {
        current(account, epoch, cx)?;
        let writes = match job {
            Job::Seed => {
                let request = cx.update(|cx| seed_church(account, cx));
                if let Some((request, church)) = request {
                    let result = request.await;
                    current(account, epoch, cx)?;
                    result.map_err(WriteError::Backend)?;
                    cx.update(|cx| {
                        account.update(cx, |a, cx| a.update_settings(|s| s.church = church, cx))
                    });
                }
                continue;
            }
            Job::Write(write) => vec![write],
            Job::LeaveTour => {
                // Read readiness when this effect runs, after any earlier write.
                let ready = cx.update(|cx| {
                    let a = account.read(cx);
                    can_enter_app(a.session()) && a.license().is_some_and(|l| l.activated)
                });
                if ready {
                    vec![
                        OnboardingWrite::Complete(OnboardingStepId::Account),
                        OnboardingWrite::SetCurrent(OnboardingStepId::Output),
                    ]
                } else {
                    vec![OnboardingWrite::SetCurrent(OnboardingStepId::Account)]
                }
            }
        };
        for write in writes {
            current(account, epoch, cx)?;
            let request = cx.update(|cx| account.read(cx).backend().onboarding(write));
            let result = request.await;
            current(account, epoch, cx)?;
            let next = result.map_err(WriteError::Backend)?;
            state = Some(next);
        }
    }
    Ok(state)
}

type SeedRequest = (
    crate::Reply<Result<(), String>>,
    kairo_domain::settings::ChurchProfile,
);

fn seed_church(account: &Entity<Account>, cx: &gpui_kit::App) -> Option<SeedRequest> {
    let account = account.read(cx);
    let settings = account.settings()?;
    let org = account.session().org.as_ref().map(|o| o.name.as_str());
    let next = kairo_domain::cloud::seeded_church(settings, org, account.local_zone())?;
    Some((account.backend().save_church(next.clone()), next))
}
