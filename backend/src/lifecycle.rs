use crate::Store;
use std::sync::{Arc, Mutex};

pub struct RequestGuard {
    store: Store,
    request_id: String,
    reservation: String,
    started: std::time::Instant,
    state: Arc<Mutex<GuardState>>,
    events: crate::events::Events,
}
#[derive(Clone)]
struct GuardState {
    completed: bool,
}
impl RequestGuard {
    pub fn new(
        store: Store,
        request_id: String,
        reservation: String,
        events: crate::events::Events,
    ) -> Self {
        Self {
            store,
            request_id,
            reservation,
            started: std::time::Instant::now(),
            state: Arc::new(Mutex::new(GuardState { completed: false })),
            events,
        }
    }
    pub async fn complete(
        &self,
        status: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost_micros: i64,
    ) -> Result<(), String> {
        if self
            .state
            .lock()
            .map_err(|_| "request state poisoned")?
            .completed
        {
            return Ok(());
        }
        if !self
            .store
            .settle_quota_amount(
                &self.reservation,
                (input_tokens + output_tokens) as i64,
                cost_micros,
            )
            .await?
        {
            // The reservation only disappears when its client key was
            // hard-deleted (the delete clears open reservations and nulls
            // requests.client_key_id in one transaction). Skip the quota
            // settlement but still record the real outcome; a key that
            // still exists means the reservation was lost — fail loudly.
            if !self.store.request_key_deleted(&self.request_id).await? {
                return Err("reservation not found".into());
            }
        }
        self.store
            .finish_request_with_cost(
                &self.request_id,
                status,
                input_tokens,
                output_tokens,
                cost_micros,
                self.started.elapsed().as_millis() as i64,
            )
            .await?;
        self.state
            .lock()
            .map_err(|_| "request state poisoned")?
            .completed = true;
        self.events.publish(crate::events::REQUESTS_CHANGED);
        Ok(())
    }
    pub fn reservation(&self) -> &str {
        &self.reservation
    }
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if self
            .state
            .lock()
            .map(|state| state.completed)
            .unwrap_or(true)
        {
            return;
        }
        let store = self.store.clone();
        let request_id = self.request_id.clone();
        let reservation = self.reservation.clone();
        let events = self.events.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = store.settle_quota(&reservation, 0).await;
                let _ = store
                    .finish_request(&request_id, "cancelled", 0, 0, 0)
                    .await;
                events.publish(crate::events::REQUESTS_CHANGED);
            });
        }
    }
}
