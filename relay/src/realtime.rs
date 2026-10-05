use std::sync::Arc;

use tokio::sync::broadcast;

use crate::types::Batch;

#[derive(Clone)]
pub struct Realtime {
    tx: broadcast::Sender<Arc<Batch>>,
}

impl Realtime {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Batch>> {
        self.tx.subscribe()
    }

    pub fn publish(&self, batch: Batch) {
        let _ = self.tx.send(Arc::new(batch));
    }
}

impl Default for Realtime {
    fn default() -> Self {
        Self::new()
    }
}
