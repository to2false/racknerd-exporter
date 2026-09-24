pub mod api;
pub mod config;
pub mod metrics;

use api::ApiClient;
use axum::{Router, extract::State, http::header, routing::get};
use metrics::Snapshot;
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;

pub struct Exporter {
    api: ApiClient,
    // One in-flight request, shared by concurrent Prometheus scrapes.
    cache: Mutex<Option<(Instant, Snapshot)>>,
}

impl Exporter {
    pub fn new(api: ApiClient) -> Self {
        Self {
            api,
            cache: Mutex::new(None),
        }
    }

    pub async fn collect(&self) -> String {
        let mut cache = self.cache.lock().await;
        if let Some((updated, snapshot)) = cache.as_ref()
            && updated.elapsed() < self.api.cache_ttl()
        {
            return metrics::render(self.api.server(), snapshot);
        }
        let started = Instant::now();
        let result = self.api.fetch().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let (requests, failures, last_success) = cache
            .as_ref()
            .map(|(_, old)| (old.requests, old.failures, old.last_success))
            .unwrap_or((0, 0, 0.0));
        if let Err(error) = &result {
            // No secrets, response bodies, configured URL, or upstream error text.
            eprintln!("collection failed: {error}");
        }
        let snapshot = Snapshot {
            duration_seconds: started.elapsed().as_secs_f64(),
            collected_at: now,
            last_success: if result.is_ok() { now } else { last_success },
            requests: requests + 1,
            failures: failures + u64::from(result.is_err()),
            result,
        };
        let output = metrics::render(self.api.server(), &snapshot);
        *cache = Some((Instant::now(), snapshot));
        output
    }
}

pub fn router(exporter: Exporter) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async {
                "RackNerd exporter: GET /metrics for Prometheus; GET /healthz for process health.\n"
            }),
        )
        .route("/healthz", get(|| async { "ok\n" }))
        .route("/metrics", get(metrics_handler))
        .with_state(Arc::new(exporter))
}

async fn metrics_handler(
    State(exporter): State<Arc<Exporter>>,
) -> impl axum::response::IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "no-store"),
        ],
        exporter.collect().await,
    )
}
