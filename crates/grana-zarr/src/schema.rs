use serde_json::{Value, json};

pub(crate) const VERSION: &str = "0.6";
pub(crate) const INTRINSIC: &str = "physical";

pub(crate) fn axes() -> Vec<Value> {
    vec![
        json!({ "name": "t", "type": "time", "unit": "second" }),
        json!({ "name": "c", "type": "channel" }),
        json!({ "name": "z", "type": "space", "unit": "micrometer" }),
        json!({ "name": "y", "type": "space", "unit": "micrometer" }),
        json!({ "name": "x", "type": "space", "unit": "micrometer" }),
    ]
}

pub(crate) fn multiscales(name: &str, axes: &[Value], datasets: Vec<Value>) -> Value {
    json!([{
        "name": name,
        "coordinateSystems": [{ "name": INTRINSIC, "axes": axes }],
        "datasets": datasets,
    }])
}

pub(crate) fn dataset(path: &str, scale: &[f64], translation: &[f64]) -> Value {
    json!({
        "path": path,
        "coordinateTransformations": [{
            "type": "sequence",
            "input": { "path": path },
            "output": { "name": INTRINSIC },
            "transformations": [
                { "type": "scale", "scale": scale },
                { "type": "translation", "translation": translation },
            ],
        }],
    })
}
