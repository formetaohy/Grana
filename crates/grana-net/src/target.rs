use crate::Error;

const SCHEMES: [&str; 2] = ["http://", "https://"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    base: String,
}

impl Target {
    pub fn parse(location: &str) -> Result<Self, Error> {
        let trimmed = location.trim();
        let host = SCHEMES
            .iter()
            .find_map(|scheme| trimmed.strip_prefix(scheme))
            .ok_or_else(|| Error::Scheme {
                location: location.to_string(),
            })?;
        if trimmed
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(Error::Url {
                url: location.to_string(),
                detail: "the url carries whitespace".to_string(),
            });
        }
        if host.trim_matches('/').is_empty() {
            return Err(Error::Url {
                url: location.to_string(),
                detail: "the url names no host".to_string(),
            });
        }
        Ok(Self {
            base: format!("{}/", trimmed.trim_end_matches('/')),
        })
    }

    pub fn join(&self, key: &str) -> String {
        format!("{}{}", self.base, key.trim_start_matches('/'))
    }

    pub fn display(&self) -> &str {
        self.base.trim_end_matches('/')
    }
}
