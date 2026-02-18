use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use orrery_types::ExternalTaskResponse;

use crate::Worker;

type BoxFuture = Pin<Box<dyn Future<Output = anyhow::Result<serde_json::Value>> + Send>>;
type HandlerFn = Arc<dyn Fn(ExternalTaskResponse) -> BoxFuture + Send + Sync>;

/// Builds a [`Worker`] by registering handlers keyed on `(topic, process_definition_id)`.
///
/// Multiple registrations for the same topic are collapsed into a single
/// `subscribe` call; at runtime the per-topic dispatch closure routes each
/// incoming task to the correct handler based on `task.process_definition_id`.
pub struct WorkerFactory {
    base_url: String,
    entries: Vec<(String, String, HandlerFn)>,
}

impl WorkerFactory {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            entries: Vec::new(),
        }
    }

    /// Register `handler` for tasks on `topic` that belong to `process_definition_id`.
    pub fn register<F, Fut>(
        mut self,
        topic: impl Into<String>,
        process_definition_id: impl Into<String>,
        handler: F,
    ) -> Self
    where
        F: Fn(ExternalTaskResponse) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<serde_json::Value>> + Send + 'static,
    {
        let topic = topic.into();
        let process_definition_id = process_definition_id.into();
        tracing::debug!(topic, process_definition_id, "registered handler");
        let wrapped: HandlerFn = Arc::new(move |task| Box::pin(handler(task)));
        self.entries.push((topic, process_definition_id, wrapped));
        self
    }

    /// Consume the factory and produce a configured [`Worker`].
    pub fn build(self) -> Worker {
        // Group handlers by topic → HashMap<process_definition_id, HandlerFn>
        let mut by_topic: HashMap<String, HashMap<String, HandlerFn>> = HashMap::new();
        for (topic, proc_def_id, handler) in self.entries {
            by_topic
                .entry(topic)
                .or_default()
                .insert(proc_def_id, handler);
        }

        for (topic, handlers_by_def) in &by_topic {
            let defs: Vec<&str> = handlers_by_def.keys().map(|s| s.as_str()).collect();
            tracing::info!(topic, process_definition_ids = ?defs, "subscribed");
        }

        let mut worker = Worker::new(self.base_url);
        for (topic, handlers_by_def) in by_topic {
            let proc_def_ids: Vec<String> = handlers_by_def.keys().cloned().collect();
            let handlers_by_def = Arc::new(handlers_by_def);

            worker = worker.subscribe(topic, proc_def_ids, move |task| {
                let handlers_by_def = handlers_by_def.clone();
                async move {
                    let id = &task.process_definition_id;
                    match handlers_by_def.get(id) {
                        Some(handler) => handler(task).await,
                        None => {
                            anyhow::bail!("no handler registered for process_definition_id: {id}")
                        }
                    }
                }
            });
        }
        worker
    }
}
