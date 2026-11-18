use std::sync::{Arc, OnceLock};
use std::thread::sleep;
use std::time::Duration;

use ureq::Agent;

use crate::Error;

const ATTEMPTS: u32 = 3;
const BACKOFF: Duration = Duration::from_millis(200);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
const BODY_LIMIT: u64 = 1 << 30;
const AGENT: &str = concat!("grana/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Span {
    From(u64),
    Between(u64, u64),
}

impl Span {
    fn header(self) -> String {
        match self {
            Self::From(start) => format!("bytes={start}-"),
            Self::Between(start, end) => format!("bytes={start}-{end}"),
        }
    }
}

pub struct Client {
    agent: Agent,
}

#[derive(Clone, Debug)]
pub struct Probe {
    pub size: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Client {
    pub fn new() -> Self {
        let config = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(BODY_TIMEOUT))
            .user_agent(AGENT)
            .build();
        Self {
            agent: Agent::new_with_config(config),
        }
    }

    pub fn shared() -> Arc<Self> {
        static SHARED: OnceLock<Arc<Client>> = OnceLock::new();
        Arc::clone(SHARED.get_or_init(|| Arc::new(Self::new())))
    }

    pub fn get(&self, url: &str, span: Option<Span>) -> Result<Option<Vec<u8>>, Error> {
        let mut attempt = 0;
        loop {
            let request = match span {
                Some(span) => self.agent.get(url).header("Range", span.header()),
                None => self.agent.get(url),
            };
            match request.call() {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 404 | 410) {
                        return Ok(None);
                    }
                    if transient(status) && retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    if !(200..300).contains(&status) {
                        return Err(Error::Status {
                            url: url.to_string(),
                            status,
                        });
                    }
                    let bytes = response
                        .body_mut()
                        .with_config()
                        .limit(BODY_LIMIT)
                        .read_to_vec()
                        .map_err(|error| Error::Transport {
                            url: url.to_string(),
                            detail: error.to_string(),
                        })?;
                    return Ok(Some(bytes));
                }
                Err(error) => {
                    if retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport {
                        url: url.to_string(),
                        detail: error.to_string(),
                    });
                }
            }
        }
    }

    pub fn probe(&self, url: &str) -> Result<Option<Probe>, Error> {
        let mut attempt = 0;
        loop {
            match self.agent.head(url).call() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 404 | 410) {
                        return Ok(None);
                    }
                    if transient(status) && retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    if matches!(status, 403 | 405 | 501) {
                        return self.probe_ranged(url);
                    }
                    if !(200..300).contains(&status) {
                        return Err(Error::Status {
                            url: url.to_string(),
                            status,
                        });
                    }
                    return Ok(Some(probed(&response)));
                }
                Err(error) => {
                    if retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport {
                        url: url.to_string(),
                        detail: error.to_string(),
                    });
                }
            }
        }
    }

    fn probe_ranged(&self, url: &str) -> Result<Option<Probe>, Error> {
        let mut attempt = 0;
        loop {
            match self.agent.get(url).header("Range", "bytes=0-0").call() {
                Ok(mut response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 404 | 410) {
                        return Ok(None);
                    }
                    if transient(status) && retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    if !(200..300).contains(&status) {
                        return Err(Error::Status {
                            url: url.to_string(),
                            status,
                        });
                    }
                    let mut probe = probed(&response);
                    if probe.size.is_none() {
                        probe.size = response
                            .headers()
                            .get("content-range")
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.rsplit('/').next())
                            .and_then(|value| value.parse().ok());
                    }
                    let _ = response
                        .body_mut()
                        .with_config()
                        .limit(1 << 20)
                        .read_to_vec();
                    return Ok(Some(probe));
                }
                Err(error) => {
                    if retry(attempt) {
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Transport {
                        url: url.to_string(),
                        detail: error.to_string(),
                    });
                }
            }
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Self::new()
    }
}

fn transient(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
}

fn retry(attempt: u32) -> bool {
    if attempt + 1 >= ATTEMPTS {
        return false;
    }
    sleep(BACKOFF * (1u32 << attempt));
    true
}

fn probed(response: &ureq::http::Response<ureq::Body>) -> Probe {
    Probe {
        size: header(response, "content-length").and_then(|value| value.parse().ok()),
        etag: header(response, "etag"),
        last_modified: header(response, "last-modified"),
    }
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
