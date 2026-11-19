pub(super) mod hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{value:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let text = String::deserialize(deserializer)?;
        u64::from_str_radix(&text, 16).map_err(serde::de::Error::custom)
    }
}

pub(super) mod fingerprint {
    use grana_volume::Fingerprint;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Fingerprint, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.hex())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Fingerprint, D::Error> {
        let text = String::deserialize(deserializer)?;
        Fingerprint::from_hex(&text).ok_or_else(|| {
            serde::de::Error::custom("a content fingerprint holds 64 hexadecimal digits")
        })
    }
}

pub(super) mod maybe_hex {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_str(&format!("{value:016x}")),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Option::<String>::deserialize(deserializer)?
            .map(|text| u64::from_str_radix(&text, 16).map_err(serde::de::Error::custom))
            .transpose()
    }
}
