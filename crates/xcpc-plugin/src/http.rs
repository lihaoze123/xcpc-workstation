use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{PluginError, Result};

const DEFAULT_HTTP_TIMEOUT_MS: u64 = 15_000;
const MAX_HTTP_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

fn default_method() -> String {
    "GET".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub trait HttpClient: Send + Sync {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse>;

    /// Downloads raw response bytes from a URL (used for `.zip` plugin packages).
    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let resp = self.send(&HttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: Some(DEFAULT_HTTP_TIMEOUT_MS),
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(PluginError::Http(format!(
                "HTTP {} downloading {url}",
                resp.status
            )));
        }
        Ok(resp.body.into_bytes())
    }
}

/// Real synchronous HTTP client backed by `ureq`.
#[derive(Debug, Default)]
pub struct UreqHttpClient;

impl UreqHttpClient {
    pub fn new() -> Self {
        Self
    }
}

impl HttpClient for UreqHttpClient {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        let timeout_ms = req
            .timeout_ms
            .unwrap_or(DEFAULT_HTTP_TIMEOUT_MS)
            .clamp(500, MAX_HTTP_TIMEOUT_MS);
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(timeout_ms))
            .redirects(0)
            .user_agent("XCPC-Workstation/0.3")
            .build();

        let method = req.method.to_ascii_uppercase();
        let mut request = agent.request(&method, &req.url);
        for (k, v) in &req.headers {
            request = request.set(k, v);
        }

        let response = if let Some(ref body) = req.body {
            request.send_string(body)
        } else {
            request.call()
        };

        let resp = match response {
            Ok(resp) => resp,
            Err(ureq::Error::Status(_, resp)) => resp,
            Err(ureq::Error::Transport(t)) => return Err(PluginError::Http(t.to_string())),
        };

        let status = resp.status();
        let mut headers = BTreeMap::new();
        for name in resp.headers_names() {
            if let Some(val) = resp.header(&name) {
                headers.insert(name.to_ascii_lowercase(), val.to_string());
            }
        }
        let body = resp
            .into_string()
            .map_err(|e| PluginError::Http(format!("failed to read response body: {e}")))?;

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        const MAX_REDIRECTS: usize = 5;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(DEFAULT_HTTP_TIMEOUT_MS))
            .redirects(0)
            .user_agent("XCPC-Workstation/0.3")
            .build();

        let mut current_url = url.to_string();
        for hop in 0..=MAX_REDIRECTS {
            crate::manager::validate_repo_url(&current_url)?;
            let resp = match agent.get(&current_url).call() {
                Ok(resp) => resp,
                Err(ureq::Error::Status(_, resp)) => resp,
                Err(ureq::Error::Transport(t)) => return Err(PluginError::Http(t.to_string())),
            };

            let status = resp.status();
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let Some(loc) = resp.header("location").filter(|s| !s.trim().is_empty()) else {
                    return Err(PluginError::Http(format!(
                        "HTTP {status} redirect without Location downloading {current_url}"
                    )));
                };
                if hop == MAX_REDIRECTS {
                    return Err(PluginError::Http(format!(
                        "too many redirects downloading {url}"
                    )));
                }
                let base = url::Url::parse(&current_url).map_err(|e| {
                    PluginError::Http(format!("invalid download url '{current_url}': {e}"))
                })?;
                let next = base.join(loc.trim()).map_err(|e| {
                    PluginError::Http(format!("invalid redirect location '{loc}': {e}"))
                })?;
                current_url = next.to_string();
                continue;
            }

            if !(200..300).contains(&status) {
                return Err(PluginError::Http(format!(
                    "HTTP {status} downloading {current_url}"
                )));
            }

            let mut buf = Vec::new();
            resp.into_reader()
                .take(64 * 1024 * 1024)
                .read_to_end(&mut buf)
                .map_err(|e| PluginError::Http(format!("failed to read binary body: {e}")))?;
            return Ok(buf);
        }

        Err(PluginError::Http(format!(
            "too many redirects downloading {url}"
        )))
    }
}

type MockHandler = Arc<dyn Fn(&HttpRequest) -> Result<HttpResponse> + Send + Sync>;
type MockBytesHandler = Arc<dyn Fn(&str) -> Result<Vec<u8>> + Send + Sync>;

/// Injectable mock HTTP client for deterministic tests.
#[derive(Clone)]
pub struct MockHttpClient {
    handler: MockHandler,
    bytes_handler: Option<MockBytesHandler>,
    recorded: Arc<Mutex<Vec<HttpRequest>>>,
}

impl MockHttpClient {
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&HttpRequest) -> Result<HttpResponse> + Send + Sync + 'static,
    {
        Self {
            handler: Arc::new(handler),
            bytes_handler: None,
            recorded: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_bytes_handler<F>(mut self, handler: F) -> Self
    where
        F: Fn(&str) -> Result<Vec<u8>> + Send + Sync + 'static,
    {
        self.bytes_handler = Some(Arc::new(handler));
        self
    }

    pub fn recorded_requests(&self) -> Vec<HttpRequest> {
        self.recorded.lock().unwrap().clone()
    }
}

impl HttpClient for MockHttpClient {
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse> {
        self.recorded.lock().unwrap().push(req.clone());
        (self.handler)(req)
    }

    fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        if let Some(ref bh) = self.bytes_handler {
            return bh(url);
        }
        let resp = self.send(&HttpRequest {
            url: url.to_string(),
            method: "GET".to_string(),
            headers: BTreeMap::new(),
            body: None,
            timeout_ms: Some(DEFAULT_HTTP_TIMEOUT_MS),
        })?;
        if !(200..300).contains(&resp.status) {
            return Err(PluginError::Http(format!(
                "HTTP {} downloading {url}",
                resp.status
            )));
        }
        Ok(resp.body.into_bytes())
    }
}
