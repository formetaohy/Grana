use crate::Error;

const RELEASE: &str = "releases/latest/download/latest.toml";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    root: String,
    origin: String,
}

impl Source {
    pub fn github() -> Self {
        Self::at(env!("CARGO_PKG_REPOSITORY")).expect("the repository names an https origin")
    }

    pub fn at(location: &str) -> Result<Self, Error> {
        let trimmed = location.trim().trim_end_matches('/');
        let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
        let (scheme, rest) = trimmed.split_once("://").ok_or_else(|| Error::Source {
            location: location.to_string(),
            detail: "the url names no scheme".to_string(),
        })?;
        if !matches!(scheme, "http" | "https") {
            return Err(Error::Source {
                location: location.to_string(),
                detail: format!("the scheme `{scheme}` is not http or https"),
            });
        }
        if rest.contains(|character: char| character.is_whitespace() || character.is_control()) {
            return Err(Error::Source {
                location: location.to_string(),
                detail: "the url carries whitespace".to_string(),
            });
        }
        let host = rest.split('/').next().unwrap_or_default();
        if host.is_empty() {
            return Err(Error::Source {
                location: location.to_string(),
                detail: "the url names no host".to_string(),
            });
        }
        Ok(Self {
            root: trimmed.to_string(),
            origin: format!("{scheme}://{host}"),
        })
    }

    pub fn manifest(&self) -> String {
        format!("{}/{RELEASE}", self.root)
    }

    pub fn admits(&self, url: &str) -> bool {
        url.strip_prefix(&self.origin)
            .is_some_and(|rest| rest.starts_with('/'))
    }
}
