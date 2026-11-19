use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Deserialize, Serialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>", serialize = "T: Serialize"))]
struct Slot<T> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<T>,
}

pub fn serialize<S, T>(values: &[Option<T>], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Clone + Serialize,
{
    values
        .iter()
        .cloned()
        .map(|value| Slot { value })
        .collect::<Vec<_>>()
        .serialize(serializer)
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let slots = Vec::<Slot<T>>::deserialize(deserializer)?;
    Ok(slots.into_iter().map(|slot| slot.value).collect())
}
