use std::io::Read;
use std::sync::{Arc, OnceLock};
use std::thread::sleep;
use std::time::Duration;

use ureq::Agent;

use crate::Error;

const ATTEMPTS: u32 = 4;
const BACKOFF: Duration = Duration::from_millis(100);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const BODY_TIMEOUT: Duration = Duration::from_secs(120);
const BODY_LIMIT: u64 = 1 << 30;
const AGENT: &str = concat!("grana/", env!("CARGO_PKG_VERSION"));
const CHUNK: usize = 1 << 16;

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

    fn advance(self, bytes: u64) -> Self {
        match self {
            Self::From(start) => Self::From(start + bytes),
            Self::Between(start, end) => Self::Between((start + bytes).min(end), end),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    pub bytes: Vec<u8>,
    pub resumed: u64,
}

pub struct Client {
    agent: Agent,
}

enum Outcome<T> {
    Ready(T),
    Retry(Error, Vec<u8>),
    Refuse(Error),
}

enum Body {
    Complete,
    Truncated(Error),
    Refused(Error),
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

    pub fn get(&self, url: &str, span: Option<Span>) -> Result<Option<Fetch>, Error> {
        attempt(|held| self.drain(url, span, held))
    }

    pub fn probe(&self, url: &str) -> Result<Presence, Error> {
        attempt(|_| self.head(url))
    }

    fn drain(&self, url: &str, wanted: Option<Span>, held: Vec<u8>) -> Outcome<Option<Fetch>> {
        let base = held.len() as u64;
        if let Some(length) = wanted.and_then(Span::length)
            && base >= length
        {
            return Outcome::Ready(Some(Fetch {
                bytes: held,
                resumed: base,
            }));
        }
        let resume = wanted.is_none() && base > 0;
        let span = match wanted {
            Some(span) => Some(span.advance(base)),
            None if resume => Some(Span::From(base)),
            None => None,
        };
        let request = match span {
            Some(span) => self
                .agent
                .get(url)
                .header("Range", span.header())
                .header("Accept-Encoding", "identity"),
            None => self.agent.get(url),
        };
        let mut response = match request.call() {
            Ok(response) => response,
            Err(error) => return classify(url, error, held),
        };
        let status = response.status().as_u16();
        if matches!(status, 404 | 410) {
            return Outcome::Ready(None);
        }
        if retryable(status) {
            return Outcome::Retry(status_error(url, status), held);
        }
        if !(200..300).contains(&status) {
            return Outcome::Refuse(status_error(url, status));
        }
        let (span, mut held, base) = match (span, status) {
            (Some(_), 206) => (span, held, base),
            (Some(_), _) if wanted.is_some() => {
                return Outcome::Refuse(Error::Range {
                    url: url.to_string(),
                    detail: "the server answered the byte range with a whole response".to_string(),
                });
            }
            (Some(_), _) => (None, Vec::new(), 0),
            (None, _) => (None, held, base),
        };
        let want = match span {
            Some(span) => match expected(url, &response, span) {
                Ok(_) => span.length().or_else(|| {
                    range_of(&response)
                        .and_then(|range| range.total)
                        .map(|total| total.saturating_sub(span.start()))
                }),
                Err(error) => return Outcome::Refuse(error),
            },
            None => header(&response, "content-length").and_then(|value| value.parse().ok()),
        };
        match read(&mut response, url, want, &mut held) {
            Body::Complete => Outcome::Ready(Some(Fetch {
                bytes: held,
                resumed: base,
            })),
            Body::Truncated(error) => Outcome::Retry(error, held),
            Body::Refused(error) => Outcome::Refuse(error),
        }
    }

    fn head(&self, url: &str) -> Outcome<Presence> {
        let response = match self.agent.head(url).call() {
            Ok(response) => response,
            Err(error) => return classify(url, error, Vec::new()),
        };
        let status = response.status().as_u16();
        if matches!(status, 404 | 410) {
            return Outcome::Ready(Presence::Missing);
        }
        if retryable(status) {
            return Outcome::Retry(status_error(url, status), Vec::new());
        }
        if matches!(status, 403 | 405 | 501) {
            return self.head_ranged(url);
        }
        if !(200..300).contains(&status) {
            return Outcome::Refuse(status_error(url, status));
        }
        let size = header(&response, "content-length").and_then(|value| value.parse().ok());
        Outcome::Ready(presence(&response, size))
    }

    fn head_ranged(&self, url: &str) -> Outcome<Presence> {
        let response = match self
            .agent
            .get(url)
            .header("Range", Span::Between(0, 0).header())
            .header("Accept-Encoding", "identity")
            .call()
        {
            Ok(response) => response,
            Err(error) => return classify(url, error, Vec::new()),
        };
        let status = response.status().as_u16();
        if matches!(status, 404 | 410) {
            return Outcome::Ready(Presence::Missing);
        }
        if retryable(status) {
            return Outcome::Retry(status_error(url, status), Vec::new());
        }
        if !(200..300).contains(&status) {
            return Outcome::Refuse(status_error(url, status));
        }
        let size = match status {
            206 => range_of(&response).and_then(|range| range.total),
            _ => header(&response, "content-length").and_then(|value| value.parse().ok()),
        };
        Outcome::Ready(presence(&response, size))
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

fn attempt<T>(mut call: impl FnMut(Vec<u8>) -> Outcome<T>) -> Result<T, Error> {
    let mut held = Vec::new();
    for round in 0..ATTEMPTS {
        match call(held) {
            Outcome::Ready(value) => return Ok(value),
            Outcome::Refuse(error) => return Err(error),
            Outcome::Retry(error, retained) => {
                if round + 1 == ATTEMPTS {
                    return Err(error);
                }
                held = retained;
                sleep(BACKOFF * (1u32 << round));
            }
        }
    }
    unreachable!("the retry loop returns on its final round")
}

fn classify<T>(url: &str, error: ureq::Error, held: Vec<u8>) -> Outcome<T> {
    if matches!(error, ureq::Error::StatusCode(_)) {
        return Outcome::Refuse(Error::Transport {
            url: url.to_string(),
            detail: error.to_string(),
        });
    }
    Outcome::Retry(
        Error::Transport {
            url: url.to_string(),
            detail: error.to_string(),
        },
        held,
    )
}

fn status_error(url: &str, status: u16) -> Error {
    Error::Status {
        url: url.to_string(),
        status,
    }
}

fn presence(response: &ureq::http::Response<ureq::Body>, size: Option<u64>) -> Presence {
    Presence::Object {
        size,
        etag: header(response, "etag"),
        last_modified: header(response, "last-modified"),
    }
}

fn expected(
    url: &str,
    response: &ureq::http::Response<ureq::Body>,
    span: Span,
) -> Result<Option<u64>, Error> {
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
    Ok(expected)
}

fn read(
    response: &mut ureq::http::Response<ureq::Body>,
    url: &str,
    want: Option<u64>,
    held: &mut Vec<u8>,
) -> Body {
    let already = held.len() as u64;
    if already > BODY_LIMIT || want.is_some_and(|want| already + want > BODY_LIMIT) {
        return Body::Refused(Error::Size {
            url: url.to_string(),
        });
    }
    let guard = want.map_or(BODY_LIMIT, |want| want + 1);
    let mut reader = response.body_mut().with_config().limit(guard).reader();
    let mut buffer = [0u8; CHUNK];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => held.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Body::Truncated(transport(url, error.into())),
        }
    }
    let gained = held.len() as u64 - already;
    match want {
        Some(want) if gained != want => Body::Truncated(Error::Range {
            url: url.to_string(),
            detail: format!("the server sent {gained} of the {want} bytes still awaited"),
        }),
        _ if held.len() as u64 > BODY_LIMIT => Body::Refused(Error::Size {
            url: url.to_string(),
        }),
        _ => Body::Complete,
    }
}

#[derive(Clone, Copy, Debug)]
struct Range {
    start: u64,
    end: u64,
    total: Option<u64>,
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

fn retryable(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 500..=599)
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
