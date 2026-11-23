//! Live workspace change notifications for the console.
//!
//! The management API pushes domain-level hints over Server-Sent Events so
//! every open console refreshes the affected panels without polling. Events
//! carry domain names only — never data — and the SSE endpoint is
//! authenticated by the same session middleware as the rest of `/api/v1`.

use tokio::sync::broadcast;

/// Request records and analytics aggregates change together because every
/// recorded request feeds both panels.
pub const REQUESTS_CHANGED: &[&str] = &["requests", "analytics"];

/// Burst capacity; a subscriber that falls further behind receives a
/// "refresh everything" hint instead of individual events.
const CHANNEL_CAPACITY: usize = 256;

/// Fan-out of workspace change hints to the open SSE subscribers.
#[derive(Clone)]
pub struct Events {
    sender: broadcast::Sender<Vec<&'static str>>,
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

impl Events {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<&'static str>> {
        self.sender.subscribe()
    }

    /// Publishes a domain hint; an empty slice asks subscribers to refresh
    /// every workspace domain.
    pub fn publish(&self, domains: &[&'static str]) {
        // Err only means nobody is listening right now.
        let _ = self.sender.send(domains.to_vec());
    }
}
