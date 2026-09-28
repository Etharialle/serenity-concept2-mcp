//! Bounded, read-only access to the authenticated user's Concept2 Logbook.

use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, RETRY_AFTER},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::{sync::Semaphore, time};

const ACCEPT_VALUE: &str = "application/vnd.c2logbook.v1+json";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Only the documented Concept2 environments are available to callers.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ApiEnvironment {
    #[default]
    Production,
    Development,
}

impl ApiEnvironment {
    fn origin(self) -> &'static str {
        match self {
            Self::Production => "https://log.concept2.com/",
            Self::Development => "https://log-dev.concept2.com/",
        }
    }
}

/// Errors deliberately contain no upstream body, URL, or credential.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ApiError {
    #[error("Concept2 rejected the access token; check or replace the configured token")]
    Authentication,
    #[error("The configured token does not have permission to read this resource")]
    Forbidden,
    #[error("The requested Concept2 resource was not found")]
    NotFound,
    #[error("The request parameters are invalid or were rejected by Concept2")]
    Validation,
    #[error("Concept2 is rate limiting requests; try again later")]
    RateLimited,
    #[error("The Concept2 request exceeded its time limit")]
    Timeout,
    #[error("Concept2 could not complete the request; try again later")]
    Upstream,
    #[error("Concept2 returned an invalid or inconsistent response")]
    InvalidResponse,
    #[error("The Concept2 response exceeded the allowed size")]
    ResponseTooLarge,
    #[error("The connection to Concept2 failed")]
    Transport,
    #[error("The API configuration is invalid; check the configured access token")]
    Configuration,
    #[error("Concept2 returned a redirect, which this client does not follow")]
    UnexpectedRedirect,
}

impl ApiError {
    /// Stable machine-readable code, independent of the user-facing message.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Validation => "validation",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Upstream => "upstream",
            Self::InvalidResponse => "invalid_response",
            Self::ResponseTooLarge => "response_too_large",
            Self::Transport => "transport",
            Self::Configuration => "configuration",
            Self::UnexpectedRedirect => "unexpected_redirect",
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkoutQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub equipment: Option<String>,
    pub page: u32,
    pub page_size: u32,
}

impl Default for WorkoutQuery {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            equipment: None,
            page: 1,
            page_size: 50,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Pagination {
    pub total: u64,
    pub count: u32,
    pub per_page: u32,
    pub current_page: u32,
    pub total_pages: u32,
}

#[derive(Clone, Debug)]
pub struct WorkoutPage {
    pub data: Vec<Value>,
    pub pagination: Pagination,
}

#[async_trait]
pub trait LogbookApi: Send + Sync {
    async fn profile(&self) -> Result<Value, ApiError>;
    async fn workouts(&self, query: &WorkoutQuery) -> Result<WorkoutPage, ApiError>;
    async fn workout(&self, id: u64) -> Result<Value, ApiError>;
    async fn strokes(&self, id: u64) -> Result<Value, ApiError>;
}

#[derive(Clone, Copy)]
struct RequestPolicy {
    attempt_timeout: Duration,
    total_timeout: Duration,
    retry_base: Duration,
    max_retry_delay: Duration,
    max_attempts: u32,
    max_response_bytes: usize,
}

impl Default for RequestPolicy {
    fn default() -> Self {
        Self {
            attempt_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(25),
            retry_base: Duration::from_millis(250),
            max_retry_delay: Duration::from_secs(3),
            max_attempts: 3,
            max_response_bytes: MAX_RESPONSE_BYTES,
        }
    }
}

/// Cloneable client; clones share the HTTP pool and the concurrency limit.
#[derive(Clone)]
pub struct ApiClient {
    http: Client,
    origin: Url,
    permits: Arc<Semaphore>,
    policy: RequestPolicy,
}

impl fmt::Debug for ApiClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Do not expose the HTTP client or its default authorization header.
        formatter.debug_struct("ApiClient").finish_non_exhaustive()
    }
}

impl ApiClient {
    pub fn new(token: String, environment: ApiEnvironment) -> Result<Self, ApiError> {
        let origin = Url::parse(environment.origin()).map_err(|_| ApiError::Configuration)?;
        Self::build(token, origin, RequestPolicy::default(), true)
    }

    fn build(
        token: String,
        origin: Url,
        policy: RequestPolicy,
        https_only: bool,
    ) -> Result<Self, ApiError> {
        if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(ApiError::Configuration);
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| ApiError::Configuration)?;
        authorization.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(ACCEPT, HeaderValue::from_static(ACCEPT_VALUE));

        let mut builder = Client::builder()
            .default_headers(headers)
            .https_only(https_only)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(3))
            .timeout(policy.attempt_timeout)
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ));
        // The loopback test server must not use an ambient HTTP proxy.
        if !https_only {
            builder = builder.no_proxy();
        }
        let http = builder.build().map_err(|_| ApiError::Configuration)?;
        Ok(Self {
            http,
            origin,
            permits: Arc::new(Semaphore::new(2)),
            policy,
        })
    }

    async fn get_json(&self, path: &str, query: &[(String, String)]) -> Result<Value, ApiError> {
        // The total deadline includes time spent waiting for a concurrency slot.
        time::timeout(self.policy.total_timeout, async {
            let _permit = self
                .permits
                .acquire()
                .await
                .map_err(|_| ApiError::Transport)?;
            let url = self
                .origin
                .join(path)
                .map_err(|_| ApiError::Configuration)?;
            for attempt in 0..self.policy.max_attempts {
                let result = self.http.get(url.clone()).query(query).send().await;
                let (error, retry_after, retryable) = match result {
                    Ok(response) => {
                        let status = response.status();
                        if status.is_success() {
                            match read_json(response, self.policy.max_response_bytes).await {
                                Ok(value) => return Ok(value),
                                Err(error) => {
                                    let retryable =
                                        matches!(error, ApiError::Transport | ApiError::Timeout);
                                    (error, None, retryable)
                                }
                            }
                        } else {
                            let retry_after = response.headers().get(RETRY_AFTER).cloned();
                            let error = error_for_status(status);
                            let retryable = matches!(status.as_u16(), 429 | 500 | 502 | 503 | 504);
                            // Error bodies are deliberately not read or included in diagnostics.
                            (error, retry_after, retryable)
                        }
                    }
                    Err(error) => (transport_error(&error), None, true),
                };
                if !retryable || attempt + 1 == self.policy.max_attempts {
                    return Err(error);
                }
                let fallback = self.policy.retry_base * (1 << attempt);
                let Some(delay) =
                    retry_delay(retry_after.as_ref(), fallback, self.policy.max_retry_delay)
                else {
                    // Do not shorten a server-requested delay and retry too early.
                    return Err(error);
                };
                time::sleep(delay).await;
            }
            Err(ApiError::Upstream)
        })
        .await
        .map_err(|_| ApiError::Timeout)?
    }

    async fn get_data(&self, path: &str) -> Result<Value, ApiError> {
        let mut response = self.get_json(path, &[]).await?;
        response
            .as_object_mut()
            .and_then(|object| object.remove("data"))
            .ok_or(ApiError::InvalidResponse)
    }
}

#[async_trait]
impl LogbookApi for ApiClient {
    async fn profile(&self) -> Result<Value, ApiError> {
        let data = self.get_data("api/users/me").await?;
        if !data.is_object() {
            return Err(ApiError::InvalidResponse);
        }
        Ok(data)
    }

    async fn workouts(&self, query: &WorkoutQuery) -> Result<WorkoutPage, ApiError> {
        if query.page == 0 || !(1..=250).contains(&query.page_size) {
            return Err(ApiError::Validation);
        }
        let mut parameters = vec![
            ("page".to_owned(), query.page.to_string()),
            ("number".to_owned(), query.page_size.to_string()),
        ];
        for (name, value) in [
            ("from", &query.from),
            ("to", &query.to),
            ("type", &query.equipment),
        ] {
            if let Some(value) = value {
                if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
                    return Err(ApiError::Validation);
                }
                parameters.push((name.to_owned(), value.clone()));
            }
        }
        let response = self.get_json("api/users/me/results", &parameters).await?;
        let page: WorkoutEnvelope =
            serde_json::from_value(response).map_err(|_| ApiError::InvalidResponse)?;
        let pagination = page.meta.pagination;
        if pagination.count as usize != page.data.len()
            || pagination.per_page == 0
            || pagination.per_page > 250
            || pagination.count > pagination.per_page
            || pagination.count > query.page_size
            || pagination.current_page != query.page
            || u64::from(pagination.count) > pagination.total
            || (pagination.total > 0 && pagination.total_pages == 0)
            || page.data.iter().any(|workout| !workout.is_object())
        {
            return Err(ApiError::InvalidResponse);
        }
        Ok(WorkoutPage {
            data: page.data,
            pagination,
        })
    }

    async fn workout(&self, id: u64) -> Result<Value, ApiError> {
        if id == 0 {
            return Err(ApiError::Validation);
        }
        let data = self.get_data(&format!("api/users/me/results/{id}")).await?;
        if !data.is_object() {
            return Err(ApiError::InvalidResponse);
        }
        Ok(data)
    }

    async fn strokes(&self, id: u64) -> Result<Value, ApiError> {
        if id == 0 {
            return Err(ApiError::Validation);
        }
        let data = self
            .get_data(&format!("api/users/me/results/{id}/strokes"))
            .await?;
        if !data.is_array() {
            return Err(ApiError::InvalidResponse);
        }
        Ok(data)
    }
}

#[derive(Deserialize)]
struct WorkoutEnvelope {
    data: Vec<Value>,
    meta: PageMetadata,
}

#[derive(Deserialize)]
struct PageMetadata {
    pagination: Pagination,
}

fn transport_error(error: &reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Transport
    }
}

fn error_for_status(status: StatusCode) -> ApiError {
    match status.as_u16() {
        401 => ApiError::Authentication,
        403 => ApiError::Forbidden,
        404 => ApiError::NotFound,
        400 | 405 | 409 | 422 => ApiError::Validation,
        408 => ApiError::Timeout,
        429 => ApiError::RateLimited,
        300..=399 => ApiError::UnexpectedRedirect,
        _ => ApiError::Upstream,
    }
}

async fn read_json(mut response: Response, max_bytes: usize) -> Result<Value, ApiError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ApiError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| transport_error(&error))?
    {
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(ApiError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| ApiError::InvalidResponse)
}

fn retry_delay(
    header: Option<&HeaderValue>,
    fallback: Duration,
    cap: Duration,
) -> Option<Duration> {
    let delay = match header {
        None => fallback,
        Some(header) => {
            // A malformed Retry-After is not permission to retry immediately.
            let value = header.to_str().ok()?.trim();
            if let Ok(seconds) = value.parse::<u64>() {
                Duration::from_secs(seconds)
            } else {
                httpdate::parse_http_date(value)
                    .ok()?
                    .duration_since(std::time::SystemTime::now())
                    .unwrap_or(Duration::ZERO)
            }
        }
    };
    (delay <= cap).then_some(delay)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{collections::VecDeque, sync::Mutex};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task::{JoinHandle, JoinSet},
    };

    const FAKE_TOKEN: &str = "synthetic-token-never-a-real-credential";

    struct Reply {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
        delay: Duration,
        chunked: bool,
    }

    impl Reply {
        fn json(status: u16, body: Value) -> Self {
            Self {
                status,
                headers: vec![],
                body: body.to_string(),
                delay: Duration::ZERO,
                chunked: false,
            }
        }

        fn header(mut self, name: &str, value: &str) -> Self {
            self.headers.push((name.to_owned(), value.to_owned()));
            self
        }
    }

    struct MockServer {
        origin: Url,
        requests: Arc<Mutex<Vec<String>>>,
        task: JoinHandle<()>,
    }

    impl MockServer {
        async fn start(replies: Vec<Reply>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let origin =
                Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let saved_requests = requests.clone();
            let mut replies = VecDeque::from(replies);
            let task = tokio::spawn(async move {
                let mut connections = JoinSet::new();
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let Ok((stream, _)) = accepted else { break };
                            let reply = replies.pop_front().unwrap_or_else(|| Reply::json(500, json!({})));
                            connections.spawn(answer(stream, reply, saved_requests.clone()));
                        }
                        _ = connections.join_next(), if !connections.is_empty() => {}
                    }
                }
            });
            Self {
                origin,
                requests,
                task,
            }
        }

        fn client(&self) -> ApiClient {
            self.client_with(RequestPolicy {
                retry_base: Duration::from_millis(1),
                ..RequestPolicy::default()
            })
        }

        fn client_with(&self, policy: RequestPolicy) -> ApiClient {
            // This origin override is private and exists only in unit tests.
            ApiClient::build(FAKE_TOKEN.to_owned(), self.origin.clone(), policy, false).unwrap()
        }

        fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn answer(mut stream: TcpStream, reply: Reply, requests: Arc<Mutex<Vec<String>>>) {
        let mut request = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let Ok(count) = stream.read(&mut chunk).await else {
                return;
            };
            if count == 0 {
                return;
            }
            request.extend_from_slice(&chunk[..count]);
            if request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                break;
            }
            assert!(request.len() < 64 * 1024);
        }
        requests
            .lock()
            .unwrap()
            .push(String::from_utf8(request).unwrap());
        time::sleep(reply.delay).await;
        let mut response = format!(
            "HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nConnection: close\r\n",
            reply.status
        );
        for (name, value) in reply.headers {
            response.push_str(&format!("{name}: {value}\r\n"));
        }
        if reply.chunked {
            response.push_str("Transfer-Encoding: chunked\r\n\r\n");
            for chunk in reply.body.as_bytes().chunks(8) {
                response.push_str(&format!(
                    "{:x}\r\n{}\r\n",
                    chunk.len(),
                    std::str::from_utf8(chunk).unwrap()
                ));
            }
            response.push_str("0\r\n\r\n");
        } else {
            response.push_str(&format!(
                "Content-Length: {}\r\n\r\n{}",
                reply.body.len(),
                reply.body
            ));
        }
        let _ = stream.write_all(response.as_bytes()).await;
    }

    fn page(data: Value, total: u64, current_page: u32, per_page: u32) -> Value {
        json!({
            "data": data,
            "meta": { "pagination": {
                "total": total,
                "count": data.as_array().unwrap().len(),
                "per_page": per_page,
                "current_page": current_page,
                "total_pages": total.div_ceil(u64::from(per_page)),
                "links": { "next": "https://attacker.invalid/private" }
            }}
        })
    }

    #[tokio::test]
    async fn methods_use_only_documented_get_paths_and_unwrap_data() {
        let server = MockServer::start(vec![
            Reply::json(200, json!({ "data": { "id": 7, "extra": true } })),
            Reply::json(200, json!({ "data": { "id": 42, "distance": 2000 } })),
            Reply::json(200, json!({ "data": [{ "t": 10, "d": 20 }] })),
        ])
        .await;
        let client = server.client();
        assert_eq!(
            client.profile().await.unwrap(),
            json!({"id": 7, "extra": true})
        );
        assert_eq!(client.workout(42).await.unwrap()["distance"], 2000);
        assert_eq!(client.strokes(42).await.unwrap()[0]["t"], 10);
        let requests = server.requests();
        for (request, path) in requests.iter().zip([
            "/api/users/me",
            "/api/users/me/results/42",
            "/api/users/me/results/42/strokes",
        ]) {
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
            let headers = request.to_ascii_lowercase();
            assert!(headers.contains(&format!("authorization: bearer {FAKE_TOKEN}\r\n")));
            assert!(headers.contains(&format!("accept: {ACCEPT_VALUE}\r\n")));
        }
    }

    #[tokio::test]
    async fn query_parameters_are_encoded_and_pagination_links_are_ignored() {
        let server =
            MockServer::start(vec![Reply::json(200, page(json!([{"id": 2}]), 4, 2, 3))]).await;
        let query = WorkoutQuery {
            from: Some("2026-09-01 00:00:00".to_owned()),
            to: Some("2026-09-28 23:59:59".to_owned()),
            equipment: Some("rower&number=250".to_owned()),
            page: 2,
            page_size: 3,
        };
        let result = server.client().workouts(&query).await.unwrap();
        assert_eq!(result.pagination.current_page, 2);
        assert_eq!(result.pagination.total, 4);
        assert_eq!(result.data[0]["id"], 2);
        let requests = server.requests();
        assert_eq!(requests.len(), 1);
        let target = requests[0].split_whitespace().nth(1).unwrap();
        let url = server.origin.join(target).unwrap();
        let parameters: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(parameters.len(), 5);
        assert_eq!(parameters["number"], "3");
        assert_eq!(parameters["page"], "2");
        assert_eq!(parameters["type"], "rower&number=250");
        assert_eq!(parameters["from"], "2026-09-01 00:00:00");
        assert_eq!(parameters["to"], "2026-09-28 23:59:59");
    }

    #[tokio::test]
    async fn empty_pages_are_valid_but_inconsistent_metadata_is_rejected() {
        let mut inconsistent = page(json!([{"id": 1}]), 1, 1, 50);
        inconsistent["meta"]["pagination"]["count"] = json!(2);
        let server = MockServer::start(vec![
            Reply::json(200, page(json!([]), 0, 1, 50)),
            Reply::json(200, inconsistent),
            Reply::json(200, json!({"data": [], "meta": {}})),
        ])
        .await;
        let client = server.client();
        assert!(
            client
                .workouts(&WorkoutQuery::default())
                .await
                .unwrap()
                .data
                .is_empty()
        );
        for _ in 0..2 {
            assert_eq!(
                client.workouts(&WorkoutQuery::default()).await.unwrap_err(),
                ApiError::InvalidResponse
            );
        }
    }

    #[tokio::test]
    async fn client_errors_are_sanitized_and_not_retried() {
        for (status, expected) in [
            (401, ApiError::Authentication),
            (403, ApiError::Forbidden),
            (404, ApiError::NotFound),
            (400, ApiError::Validation),
            (422, ApiError::Validation),
            (418, ApiError::Upstream),
        ] {
            let server = MockServer::start(vec![Reply::json(
                status,
                json!({"message": FAKE_TOKEN, "email": "private@example.invalid"}),
            )])
            .await;
            let error = server.client().profile().await.unwrap_err();
            assert_eq!(error, expected);
            assert!(!format!("{error:?} {error}").contains(FAKE_TOKEN));
            assert!(!error.to_string().contains("private@example.invalid"));
            assert_eq!(server.requests().len(), 1);
        }
    }

    #[tokio::test]
    async fn redirects_never_forward_the_token() {
        let destination =
            MockServer::start(vec![Reply::json(200, json!({"data": {"id": 1}}))]).await;
        let source = MockServer::start(vec![
            Reply::json(302, json!({})).header("Location", destination.origin.as_str()),
        ])
        .await;
        assert_eq!(
            source.client().profile().await.unwrap_err(),
            ApiError::UnexpectedRedirect
        );
        assert_eq!(source.requests().len(), 1);
        assert!(destination.requests().is_empty());
    }

    #[tokio::test]
    async fn transient_failures_retry_but_never_exceed_three_attempts() {
        let server = MockServer::start(vec![
            Reply::json(429, json!({})).header("Retry-After", "0"),
            Reply::json(503, json!({})).header("Retry-After", "0"),
            Reply::json(200, json!({"data": {"id": 1}})),
        ])
        .await;
        assert_eq!(server.client().profile().await.unwrap()["id"], 1);
        assert_eq!(server.requests().len(), 3);

        let server = MockServer::start(vec![]).await;
        assert_eq!(
            server.client().profile().await.unwrap_err(),
            ApiError::Upstream
        );
        assert_eq!(server.requests().len(), 3);
    }

    #[tokio::test]
    async fn long_or_invalid_retry_after_is_not_shortened() {
        for value in ["120", "not-a-date"] {
            let server = MockServer::start(vec![
                Reply::json(429, json!({})).header("Retry-After", value),
            ])
            .await;
            assert_eq!(
                server.client().profile().await.unwrap_err(),
                ApiError::RateLimited
            );
            assert_eq!(server.requests().len(), 1);
        }
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let fallback = Duration::from_millis(250);
        let cap = Duration::from_secs(3);
        assert_eq!(
            retry_delay(Some(&HeaderValue::from_static("2")), fallback, cap),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            retry_delay(
                Some(&HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT")),
                fallback,
                cap
            ),
            Some(Duration::ZERO)
        );
        assert_eq!(retry_delay(None, fallback, cap), Some(fallback));
        assert_eq!(
            retry_delay(
                Some(&HeaderValue::from_static("99999999999999999999999999999")),
                fallback,
                cap
            ),
            None
        );
    }

    #[tokio::test]
    async fn known_and_chunked_oversized_bodies_are_bounded() {
        for chunked in [false, true] {
            let mut reply = Reply::json(200, json!({"data": {"comment": "x".repeat(100)}}));
            reply.chunked = chunked;
            let server = MockServer::start(vec![reply]).await;
            let client = server.client_with(RequestPolicy {
                max_response_bytes: 32,
                ..RequestPolicy::default()
            });
            assert_eq!(
                client.profile().await.unwrap_err(),
                ApiError::ResponseTooLarge
            );
            assert_eq!(server.requests().len(), 1);
        }
    }

    #[tokio::test]
    async fn malformed_json_and_envelopes_fail_without_exposing_contents() {
        let mut malformed = Reply::json(200, json!({}));
        malformed.body = format!("invalid-json {FAKE_TOKEN}");
        let server = MockServer::start(vec![
            malformed,
            Reply::json(200, json!({"data": null})),
            Reply::json(200, json!({"wrong": {"id": 1}})),
        ])
        .await;
        let client = server.client();
        for _ in 0..3 {
            let error = client.profile().await.unwrap_err();
            assert_eq!(error, ApiError::InvalidResponse);
            assert!(!error.to_string().contains(FAKE_TOKEN));
        }
        assert_eq!(server.requests().len(), 3);
    }

    #[tokio::test]
    async fn invalid_query_and_ids_fail_before_network_access() {
        let server = MockServer::start(vec![]).await;
        let client = server.client();
        for query in [
            WorkoutQuery {
                page: 0,
                ..WorkoutQuery::default()
            },
            WorkoutQuery {
                page_size: 251,
                ..WorkoutQuery::default()
            },
            WorkoutQuery {
                from: Some("bad\nvalue".to_owned()),
                ..WorkoutQuery::default()
            },
        ] {
            assert_eq!(
                client.workouts(&query).await.unwrap_err(),
                ApiError::Validation
            );
        }
        assert_eq!(client.workout(0).await.unwrap_err(), ApiError::Validation);
        assert_eq!(client.strokes(0).await.unwrap_err(), ApiError::Validation);
        assert!(server.requests().is_empty());
    }

    #[tokio::test]
    async fn deadline_includes_waiting_for_a_slot_and_cancellation_releases_it() {
        let server = MockServer::start(vec![]).await;
        let client = server.client_with(RequestPolicy {
            total_timeout: Duration::from_millis(30),
            ..RequestPolicy::default()
        });
        let permits = client.permits.acquire_many(2).await.unwrap();
        assert_eq!(client.profile().await.unwrap_err(), ApiError::Timeout);
        assert!(server.requests().is_empty());
        drop(permits);

        let mut slow = Reply::json(200, json!({"data": {"id": 1}}));
        slow.delay = Duration::from_secs(2);
        let server = MockServer::start(vec![slow]).await;
        let client = server.client();
        assert!(
            time::timeout(Duration::from_millis(30), client.profile())
                .await
                .is_err()
        );
        assert_eq!(client.permits.available_permits(), 2);
    }

    #[tokio::test]
    async fn slow_responses_obey_the_total_deadline() {
        let mut slow = Reply::json(200, json!({"data": {"id": 1}}));
        slow.delay = Duration::from_secs(2);
        let server = MockServer::start(vec![slow]).await;
        let client = server.client_with(RequestPolicy {
            total_timeout: Duration::from_millis(30),
            ..RequestPolicy::default()
        });
        assert_eq!(client.profile().await.unwrap_err(), ApiError::Timeout);
        assert_eq!(client.permits.available_permits(), 2);
    }

    #[test]
    fn debug_output_and_configuration_errors_never_include_secrets() {
        let client = ApiClient::new(FAKE_TOKEN.to_owned(), ApiEnvironment::Production).unwrap();
        assert!(!format!("{client:?}").contains(FAKE_TOKEN));
        for token in ["", " secret", "secret\nheader", "secret token", "秘密"] {
            assert_eq!(
                ApiClient::new(token.to_owned(), ApiEnvironment::Production).unwrap_err(),
                ApiError::Configuration
            );
        }
    }
}
