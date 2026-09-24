use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use racknerd_exporter::{
    Exporter,
    api::{ApiClient, ApiError, MAX_RESPONSE_BYTES},
    config::Config,
    router,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;

const SUCCESS: &str =
    "<ctrl><status>success</status><vmstat>online</vmstat><bw>1000,250,750,25</bw></ctrl>";

struct Mock {
    count: AtomicUsize,
    status: Mutex<StatusCode>,
    body: Mutex<String>,
    requests: Mutex<Vec<RequestRecord>>,
    delay: Duration,
}

struct RequestRecord {
    method: String,
    query: Option<String>,
    form: HashMap<String, String>,
}

struct Server {
    state: Arc<Mock>,
    url: reqwest::Url,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn mock_handler(State(mock): State<Arc<Mock>>, request: Request<Body>) -> Response {
    mock.count.fetch_add(1, Ordering::SeqCst);
    let method = request.method().to_string();
    let query = request.uri().query().map(str::to_string);
    let bytes = to_bytes(request.into_body(), 16384).await.unwrap();
    let form = serde_urlencoded::from_bytes(&bytes).unwrap();
    mock.requests.lock().unwrap().push(RequestRecord {
        method,
        query,
        form,
    });
    tokio::time::sleep(mock.delay).await;
    let status = *mock.status.lock().unwrap();
    let body = mock.body.lock().unwrap().clone();
    (status, [("location", "/redirected")], body).into_response()
}

async fn server(status: StatusCode, body: &str, delay: Duration) -> Server {
    let state = Arc::new(Mock {
        count: AtomicUsize::new(0),
        status: Mutex::new(status),
        body: Mutex::new(body.to_owned()),
        requests: Mutex::new(vec![]),
        delay,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/client/command.php",
        listener.local_addr().unwrap()
    )
    .parse()
    .unwrap();
    let app = Router::new()
        .route("/api/client/command.php", post(mock_handler))
        .route("/redirected", get(mock_handler).post(mock_handler))
        .with_state(state.clone());
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { state, url, handle }
}

fn config(mock: &Server, ttl: Duration, timeout: Duration) -> Config {
    // HTTP is only constructed directly inside tests; environment configuration requires HTTPS.
    Config {
        api_url: mock.url.clone(),
        key: "test-key+&=".into(),
        hash: "test-hash+&=".into(),
        server: "test-vps".into(),
        listen: "127.0.0.1:0".parse().unwrap(),
        timeout,
        cache_ttl: ttl,
        collect_memory_disk: false,
    }
}

#[tokio::test]
async fn concurrent_scrapes_share_one_read_only_post_and_cache() {
    let mock = server(StatusCode::OK, SUCCESS, Duration::from_millis(30)).await;
    let exporter = Arc::new(Exporter::new(
        ApiClient::new(config(
            &mock,
            Duration::from_secs(60),
            Duration::from_secs(1),
        ))
        .unwrap(),
    ));
    let mut tasks = vec![];
    for _ in 0..8 {
        let exporter = exporter.clone();
        tasks.push(tokio::spawn(async move { exporter.collect().await }));
    }
    for task in tasks {
        let metrics = task.await.unwrap();
        assert!(metrics.contains("racknerd_bandwidth_used_bytes{server=\"test-vps\"} 250"));
        assert!(metrics.contains("racknerd_bandwidth_used_ratio{server=\"test-vps\"} 0.25"));
        assert!(metrics.contains("# TYPE racknerd_bandwidth_used_bytes gauge"));
        assert!(!metrics.contains("test-key"));
        assert!(!metrics.contains("test-hash"));
    }
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 1);
    let requests = mock.state.requests.lock().unwrap();
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert!(request.query.is_none());
    assert_eq!(request.form["action"], "info");
    assert_eq!(request.form["key"], "test-key+&=");
    assert_eq!(request.form["hash"], "test-hash+&=");
    assert_eq!(request.form["bw"], "true");
    assert_eq!(request.form["status"], "true");
    assert_eq!(request.form["mem"], "false");
}

#[tokio::test]
async fn expiration_failure_and_recovery_do_not_leave_stale_values() {
    let mock = server(StatusCode::OK, SUCCESS, Duration::ZERO).await;
    let exporter = Exporter::new(
        ApiClient::new(config(
            &mock,
            Duration::from_millis(20),
            Duration::from_secs(1),
        ))
        .unwrap(),
    );
    let first = exporter.collect().await;
    let last_success = first
        .lines()
        .find(|l| l.starts_with("racknerd_last_success_timestamp_seconds{"))
        .unwrap();
    *mock.state.body.lock().unwrap() =
        "<ctrl><status>error</status><statusmsg>SECRET echoed by provider</statusmsg></ctrl>"
            .into();
    tokio::time::sleep(Duration::from_millis(30)).await;
    let failed = exporter.collect().await;
    assert!(failed.contains("racknerd_up{server=\"test-vps\"} 0"));
    assert!(failed.contains(last_success));
    assert!(!failed.contains("racknerd_bandwidth_used_bytes"));
    assert!(!failed.contains("SECRET"));
    assert_eq!(exporter.collect().await, failed);
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 2);
    *mock.state.body.lock().unwrap() = SUCCESS.replace("online", "offline");
    tokio::time::sleep(Duration::from_millis(30)).await;
    let recovered = exporter.collect().await;
    assert!(recovered.contains("racknerd_up{server=\"test-vps\"} 1"));
    assert!(recovered.contains("racknerd_vps_online{server=\"test-vps\"} 0"));
    assert!(recovered.contains("racknerd_api_failures_total{server=\"test-vps\"} 1"));
    assert!(recovered.contains("racknerd_api_requests_total{server=\"test-vps\"} 3"));
}

#[tokio::test]
async fn redirect_is_not_followed_and_http_errors_are_failures() {
    for status in [
        StatusCode::TEMPORARY_REDIRECT,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::INTERNAL_SERVER_ERROR,
    ] {
        let mock = server(status, SUCCESS, Duration::ZERO).await;
        let client = ApiClient::new(config(&mock, Duration::ZERO, Duration::from_secs(1))).unwrap();
        assert_eq!(client.fetch().await, Err(ApiError::Http(status.as_u16())));
        assert_eq!(mock.state.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn timeout_and_response_limit_bound_upstream_work() {
    let slow = server(StatusCode::OK, SUCCESS, Duration::from_secs(1)).await;
    let client = ApiClient::new(config(&slow, Duration::ZERO, Duration::from_millis(30))).unwrap();
    assert_eq!(client.fetch().await, Err(ApiError::Timeout));
    let huge = server(
        StatusCode::OK,
        &"x".repeat(MAX_RESPONSE_BYTES + 1),
        Duration::ZERO,
    )
    .await;
    let client = ApiClient::new(config(&huge, Duration::ZERO, Duration::from_secs(1))).unwrap();
    assert_eq!(client.fetch().await, Err(ApiError::TooLarge));
}

#[tokio::test]
async fn health_is_local_and_failed_collection_still_returns_prometheus_text() {
    let mock = server(StatusCode::UNAUTHORIZED, "SECRET", Duration::ZERO).await;
    let app = router(Exporter::new(
        ApiClient::new(config(
            &mock,
            Duration::from_secs(60),
            Duration::from_secs(1),
        ))
        .unwrap(),
    ));
    let health = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    assert_eq!(mock.state.count.load(Ordering::SeqCst), 0);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics?action=reboot&key=override")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    let body = String::from_utf8(
        to_bytes(response.into_body(), 100_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(body.contains("racknerd_up{server=\"test-vps\"} 0"));
    assert!(!body.contains("SECRET"));
    assert_eq!(
        mock.state.requests.lock().unwrap()[0].form["action"],
        "info"
    );
    let response = app
        .oneshot(
            Request::builder()
                .uri("/reboot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
