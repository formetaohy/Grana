#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectSetKey(String);

impl ObjectSetKey {
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        assert!(valid(&raw), "an object set key is a lowercase slug");
        Self(raw)
    }

    pub fn parse(raw: &str) -> Option<Self> {
        valid(raw).then(|| Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 96
        && key.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
        && key
            .bytes()
            .next()
            .is_some_and(|byte| byte != b'-' && byte != b'_')
}
