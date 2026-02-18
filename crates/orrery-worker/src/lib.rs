pub mod factory;
pub use factory::WorkerFactory;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use orrery_client::OrreryClient;
use orrery_types::{
    CompleteExternalTaskRequest, ExtendLockRequest, ExternalTaskResponse, FailExternalTaskRequest,
    FetchAndLockRequest, TopicSubscription,
};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
type HandlerFn =
    Arc<dyn Fn(ExternalTaskResponse) -> BoxFuture<anyhow::Result<serde_json::Value>> + Send + Sync>;

// (handler, process_definition_ids for this topic)
type HandlerEntry = (HandlerFn, Vec<String>);

#[derive(Clone)]
pub struct WorkerConfig {
    pub base_url: String,
    pub worker_id: String,
    pub lock_duration_ms: u64,
    pub request_timeout_ms: u64,
    pub max_tasks_per_fetch: u32,
    pub concurrency: usize,
}

pub struct Worker {
    pub config: WorkerConfig,
    pub handlers: HashMap<String, HandlerEntry>,
}

impl Worker {
    pub fn new(base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        let resolved = if url.is_empty() {
            std::env::var("ORRERY_URL").unwrap_or_else(|_| "http://localhost:3000".into())
        } else {
            url
        };
        Self {
            config: WorkerConfig {
                base_url: resolved,
                worker_id: format!("worker-{}", uuid_simple()),
                lock_duration_ms: 30_000,
                request_timeout_ms: 20_000,
                max_tasks_per_fetch: 1,
                concurrency: 4,
            },
            handlers: HashMap::new(),
        }
    }

    pub fn worker_id(mut self, id: impl Into<String>) -> Self {
        self.config.worker_id = id.into();
        self
    }

    pub fn lock_duration(mut self, d: Duration) -> Self {
        self.config.lock_duration_ms = d.as_millis() as u64;
        self
    }

    pub fn concurrency(mut self, n: usize) -> Self {
        self.config.concurrency = n;
        self
    }

    /// Register a handler for a topic.
    /// `process_definition_ids`: only handle tasks from these definitions; empty = any.
    pub fn subscribe<F, Fut, S>(
        mut self,
        topic: impl Into<String>,
        process_definition_ids: impl IntoIterator<Item = S>,
        handler: F,
    ) -> Self
    where
        S: AsRef<str>,
        F: Fn(ExternalTaskResponse) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<serde_json::Value>> + Send + 'static,
    {
        let wrapped: HandlerFn = Arc::new(move |task| Box::pin(handler(task)));
        let ids: Vec<String> = process_definition_ids
            .into_iter()
            .map(|s| s.as_ref().to_string())
            .collect();
        self.handlers.insert(topic.into(), (wrapped, ids));
        self
    }

    /// Run the worker loop. Blocks until a SIGTERM/SIGINT is received.
    pub async fn run(self) -> anyhow::Result<()> {
        let client = Arc::new(OrreryClient::new(&self.config.base_url));
        let config = Arc::new(self.config);
        let handlers = Arc::new(self.handlers);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.concurrency));
        let shutdown = Arc::new(tokio::sync::Notify::new());

        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown_clone.notify_waiters();
        });

        let subscriptions: Vec<TopicSubscription> = handlers
            .iter()
            .map(|(topic, (_, ids))| TopicSubscription {
                topic: topic.clone(),
                process_definition_ids: ids.clone(),
            })
            .collect();

        loop {
            tokio::select! {
                _ = shutdown.notified() => {
                    tracing::info!("worker shutting down");
                    break;
                }
                tasks = fetch_batch(&client, &config, &subscriptions) => {
                    let tasks = match tasks {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!("fetch error: {e}, backing off 2s");
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };

                    for task in tasks {
                        let permit = semaphore.clone().acquire_owned().await.unwrap();
                        let client = client.clone();
                        let config = config.clone();
                        let handlers = handlers.clone();

                        tokio::spawn(async move {
                            let _permit = permit;
                            let topic = task.topic.clone();
                            if let Some((handler, _)) = handlers.get(&topic) {
                                let hb_client = client.clone();
                                let hb_id = task.id.clone();
                                let hb_worker = config.worker_id.clone();
                                let hb_duration = config.lock_duration_ms;
                                let hb = tokio::spawn(async move {
                                    let interval = Duration::from_millis(hb_duration / 2);
                                    loop {
                                        tokio::time::sleep(interval).await;
                                        let _ = hb_client.extend_lock(&hb_id, ExtendLockRequest {
                                            worker_id: hb_worker.clone(),
                                            new_duration_ms: hb_duration,
                                        }).await;
                                    }
                                });

                                let result = handler(task.clone()).await;
                                hb.abort();

                                match result {
                                    Ok(variables) => {
                                        let _ = client.complete_external_task(&task.id,
                                            CompleteExternalTaskRequest {
                                                worker_id: config.worker_id.clone(),
                                                variables: serde_json::from_value(variables)
                                                    .unwrap_or_default(),
                                            }
                                        ).await;
                                    }
                                    Err(e) => {
                                        let _ = client.fail_external_task(&task.id,
                                            FailExternalTaskRequest {
                                                worker_id: config.worker_id.clone(),
                                                error_message: format!("{:#}", e),
                                                retries: 0,
                                                retry_timeout_ms: 0,
                                            }
                                        ).await;
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

async fn fetch_batch(
    client: &OrreryClient,
    config: &WorkerConfig,
    subscriptions: &[TopicSubscription],
) -> anyhow::Result<Vec<ExternalTaskResponse>> {
    Ok(client
        .fetch_and_lock(FetchAndLockRequest {
            worker_id: config.worker_id.clone(),
            subscriptions: subscriptions.to_vec(),
            max_tasks: config.max_tasks_per_fetch,
            lock_duration_ms: config.lock_duration_ms,
            request_timeout_ms: config.request_timeout_ms,
        })
        .await?)
}

fn uuid_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{t:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_builder_sets_worker_id() {
        let w = Worker::new("http://localhost:8080").worker_id("my-worker");
        assert_eq!(w.config.worker_id, "my-worker");
    }

    #[test]
    fn worker_builder_sets_concurrency() {
        let w = Worker::new("http://localhost:8080").concurrency(8);
        assert_eq!(w.config.concurrency, 8);
    }

    #[test]
    fn worker_builder_sets_lock_duration() {
        let w = Worker::new("http://localhost:8080").lock_duration(Duration::from_secs(60));
        assert_eq!(w.config.lock_duration_ms, 60_000);
    }

    #[test]
    fn worker_subscribe_stores_process_definition_ids() {
        let w = Worker::new("http://localhost:8080").subscribe(
            "payments",
            ["order-v1", "order-v2"],
            |_task| async { Ok(serde_json::json!({})) },
        );
        let entry = w.handlers.get("payments").expect("payments");
        assert_eq!(entry.1, vec!["order-v1", "order-v2"]);
    }

    #[test]
    fn worker_subscribe_empty_ids_means_any_definition() {
        let w = Worker::new("http://localhost:8080").subscribe(
            "shipping",
            &[] as &[&str],
            |_task| async { Ok(serde_json::json!({})) },
        );
        let entry = w.handlers.get("shipping").expect("shipping");
        assert!(entry.1.is_empty());
    }
}
