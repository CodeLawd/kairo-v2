//! Background, durable recap upload. The service archive is the queue.

use std::{future::Future, sync::Mutex, time::Duration};

use kairo_domain::{
    cloud::{SermonUploadInput, SermonUploadResult},
    services::{
        ServiceRecord, UploadStatus, next_upload_backoff_ms, should_retry_upload, to_sermon_upload,
    },
};
use kairo_store::settings::now_ms;
use tokio::{
    sync::Notify,
    time::{Instant, sleep_until},
};

use crate::{
    CloudSession,
    service_records::{RecordServices, ServiceRecords, ServiceUsage},
};

const WAKE_INTERVAL: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Schedule {
    deadline: Option<Instant>,
    last_wake: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct UploadSchedule {
    state: Mutex<Schedule>,
    changed: Notify,
}

impl UploadSchedule {
    pub(crate) fn schedule(&self, delay: Duration) {
        let mut state = self.state.lock().expect("upload schedule");
        self.set_deadline(&mut state, Instant::now() + delay);
    }

    pub(crate) fn wake(&self) {
        let mut state = self.state.lock().expect("upload schedule");
        let now = Instant::now();
        if state
            .last_wake
            .is_none_or(|previous| now.duration_since(previous) >= WAKE_INTERVAL)
        {
            state.last_wake = Some(now);
            self.set_deadline(&mut state, now);
        }
    }

    fn set_deadline(&self, state: &mut Schedule, deadline: Instant) {
        if state.deadline.is_none_or(|old| deadline < old) {
            state.deadline = Some(deadline);
            self.changed.notify_one();
        }
    }

    async fn next(&self) {
        loop {
            let deadline = {
                let mut state = self.state.lock().expect("upload schedule");
                if state
                    .deadline
                    .is_some_and(|deadline| deadline <= Instant::now())
                {
                    state.deadline = None;
                    return;
                }
                state.deadline
            };
            if let Some(deadline) = deadline {
                tokio::select! {
                    _ = self.changed.notified() => {}
                    _ = sleep_until(deadline) => {}
                }
            } else {
                self.changed.notified().await;
            }
        }
    }
}

pub(crate) trait UploadSession {
    fn can_upload(&self) -> impl Future<Output = bool> + Send;
    fn upload_sermon(
        &self,
        payload: SermonUploadInput,
    ) -> impl Future<Output = Result<SermonUploadResult, crate::ApiError>> + Send;
}

impl UploadSession for CloudSession {
    async fn can_upload(&self) -> bool {
        self.can_upload().await
    }
    async fn upload_sermon(
        &self,
        payload: SermonUploadInput,
    ) -> Result<SermonUploadResult, crate::ApiError> {
        self.upload_sermon(payload).await
    }
}

pub(crate) async fn run<S: RecordServices>(
    records: &ServiceRecords<S>,
    session: &impl UploadSession,
) {
    loop {
        records.upload_schedule().next().await;
        let waiting = records.queued().await;
        if waiting.is_empty() {
            continue;
        }
        if session.can_upload().await {
            for record in waiting {
                if !session.can_upload().await {
                    break;
                }
                if upload_due(&record, now_ms()) {
                    upload_one(records, session, record).await;
                }
            }
        }
        if !records.queued().await.is_empty() {
            records.upload_schedule().schedule(CHECK_INTERVAL);
        }
    }
}

fn upload_due(record: &ServiceRecord, now: f64) -> bool {
    record.upload.status != UploadStatus::Failed
        || record.upload.last_attempt_at.is_none_or(|attempt| {
            now - attempt >= next_upload_backoff_ms(record.upload.attempts as usize) as f64
        })
}

async fn upload_one<S: RecordServices>(
    records: &ServiceRecords<S>,
    session: &impl UploadSession,
    record: ServiceRecord,
) {
    let id = &record.id;
    let attempted_at = now_ms();
    let started = records
        .edit_upload(id, |state| {
            if !state.retryable
                || matches!(state.status, UploadStatus::Idle | UploadStatus::Uploaded)
            {
                return false;
            }
            state.status = UploadStatus::Uploading;
            state.last_attempt_at = Some(attempted_at);
            true
        })
        .await;
    if !started {
        return;
    }

    let payload = match to_sermon_upload(&record, attempted_at) {
        Ok(payload) => payload,
        Err(error) => {
            records
                .edit_upload(id, |state| {
                    if !is_attempt(state, attempted_at) {
                        return false;
                    }
                    state.status = UploadStatus::Failed;
                    state.error = Some(error.to_string());
                    state.retryable = false;
                    true
                })
                .await;
            return;
        }
    };

    match session.upload_sermon(payload).await {
        Ok(result) => {
            // The server keys upserts by localId. If the operator requeued
            // while this request was running, its success still published
            // the same record, so finish the queued item without reposting.
            if records
                .edit_upload(id, |state| {
                    if state.status == UploadStatus::Uploaded {
                        return false;
                    }
                    state.status = UploadStatus::Uploaded;
                    state.sermon_id = Some(result.id);
                    state.error = None;
                    state.attempts = 0.0;
                    true
                })
                .await
            {
                records.metric(ServiceUsage::RecapUploaded);
            }
        }
        Err(error) => {
            records
                .edit_upload(id, |state| {
                    if !is_attempt(state, attempted_at) {
                        return false;
                    }
                    let retryable = should_retry_upload(error.status);
                    state.status = UploadStatus::Failed;
                    state.error = Some(if retryable {
                        error.message
                    } else {
                        format!("{} This will not be retried.", error.message)
                    });
                    state.attempts += 1.0;
                    state.last_attempt_at = Some(now_ms());
                    state.retryable = retryable;
                    true
                })
                .await;
        }
    }
}

fn is_attempt(state: &kairo_domain::services::ServiceUploadState, timestamp: f64) -> bool {
    state.status == UploadStatus::Uploading && state.last_attempt_at == Some(timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wake_preempts_the_pending_retry_and_throttles_duplicates() {
        let schedule = UploadSchedule::default();
        schedule.schedule(CHECK_INTERVAL);
        schedule.wake();
        tokio::time::timeout(Duration::from_secs(1), schedule.next())
            .await
            .unwrap();
        schedule.wake();
        assert!(schedule.state.lock().unwrap().deadline.is_none());
    }
}
