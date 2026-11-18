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

    fn start(self) -> u64 {
        match self {
            Self::From(start) | Self::Between(start, _) => start,
        }
    }

    fn length(self) -> Option<u64> {
        match self {
            Self::From(_) => None,
            Self::Between(start, end) => Some(end - start + 1),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    Missing,
    Object {
        size: Option<u64>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
}

pub struct Client {
    agent: Agent,
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
                Some(span) => self
                    .agent
                    .get(url)
                    .header("Range", span.header())
                    .header("Accept-Encoding", "identity"),
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
                    let bytes = match span {
                        None => read_full(&mut response, url)?,
                        Some(span) => read_span(&mut response, url, span)?,
                    };
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

    pub fn probe(&self, url: &str) -> Result<Presence, Error> {
        let mut attempt = 0;
        loop {
            match self.agent.head(url).call() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 404 | 410) {
                        return Ok(Presence::Missing);
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
                    let size =
                        header(&response, "content-length").and_then(|value| value.parse().ok());
                    let size = match size {
                        Some(size) => Some(size),
                        None => self.probe_ranged(url)?.size(),
                    };
                    return Ok(Presence::Object {
                        size,
                        etag: header(&response, "etag"),
                        last_modified: header(&response, "last-modified"),
                    });
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

    fn probe_ranged(&self, url: &str) -> Result<Presence, Error> {
        let mut attempt = 0;
        loop {
            match self
                .agent
                .get(url)
                .header("Range", Span::Between(0, 0).header())
                .header("Accept-Encoding", "identity")
                .call()
            {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if matches!(status, 404 | 410) {
                        return Ok(Presence::Missing);
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
                    let size = match status {
                        206 => range_of(&response).and_then(|range| range.total),
                        _ => {
                            header(&response, "content-length").and_then(|value| value.parse().ok())
                        }
                    };
                    return Ok(Presence::Object {
                        size,
                        etag: header(&response, "etag"),
                        last_modified: header(&response, "last-modified"),
                    });
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

impl Presence {
    pub fn size(&self) -> Option<u64> {
        match self {
            Self::Missing => None,
            Self::Object { size, .. } => *size,
        }
    }
}

fn read_full(response: &mut ureq::http::Response<ureq::Body>, url: &str) -> Result<Vec<u8>, Error> {
    response
        .body_mut()
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_vec()
        .map_err(|error| transport(url, error))
}

#[derive(Clone, Copy, Debug)]
struct Range {
    start: u64,
    end: u64,
    total: Option<u64>,
}

fn read_span(
    response: &mut ureq::http::Response<ureq::Body>,
    url: &str,
    span: Span,
) -> Result<Vec<u8>, Error> {
    if response.status().as_u16() != 206 {
        return Err(Error::Range {
            url: url.to_string(),
            detail: "the server answered the byte range with a whole response".to_string(),
        });
    }
    let range = range_of(response).ok_or_else(|| Error::Range {
        url: url.to_string(),
        detail: "the partial response carries no usable content-range".to_string(),
    })?;
    if range.start != span.start() {
        return Err(Error::Range {
            url: url.to_string(),
            detail: format!(
                "the server answered the byte range {}-{} instead of one starting at {}",
                range.start,
                range.end,
                span.start()
            ),
        });
    }
    let expected = match span.length() {
        Some(length) => Some(length),
        None => range.total.map(|total| total.saturating_sub(range.start)),
    };
    let sent = range.end - range.start + 1;
    if let Some(expected) = expected
        && sent != expected
    {
        return Err(Error::Range {
            url: url.to_string(),
            detail: format!("the server answered {sent} of the requested {expected} bytes"),
        });
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(expected.unwrap_or(BODY_LIMIT))
        .read_to_vec()
        .map_err(|error| transport(url, error))?;
    if let Some(expected) = expected
        && bytes.len() as u64 != expected
    {
        return Err(Error::Range {
            url: url.to_string(),
            detail: format!(
                "the server sent {} of the {expected} bytes the range names",
                bytes.len()
            ),
        });
    }
    Ok(bytes)
}

fn range_of(response: &ureq::http::Response<ureq::Body>) -> Option<Range> {
    let value = header(response, "content-range")?;
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    if end < start {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        total => Some(total.parse().ok()?),
    };
    Some(Range { start, end, total })
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

fn transport(url: &str, error: ureq::Error) -> Error {
    match error {
        ureq::Error::BodyExceedsLimit(_) => Error::Size {
            url: url.to_string(),
        },
        error => Error::Transport {
            url: url.to_string(),
            detail: error.to_string(),
        },
    }
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}
