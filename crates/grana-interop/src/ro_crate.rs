use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Error;

const CONTEXT: &str = "https://w3id.org/ro/crate/1.2/context";

const PROFILE: &str = "https://w3id.org/ro/crate/1.2";

const METADATA: &str = "ro-crate-metadata.json";

const COMPLETED: &str = "http://schema.org/CompletedActionStatus";

pub struct Part {
    id: String,
    name: String,
    media: String,
    description: String,
}

pub struct Action {
    id: String,
    name: String,
    description: String,
    results: Vec<String>,
}

pub struct Crate {
    name: String,
    description: String,
    software: String,
    version: String,
    source: Option<String>,
    fingerprint: Option<(String, String)>,
    parts: Vec<Part>,
    actions: Vec<Action>,
}

impl Crate {
    pub fn new(name: &str, description: &str, software: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            software: software.to_string(),
            version: version.to_string(),
            source: None,
            fingerprint: None,
            parts: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn source(&mut self, path: &Path) -> &mut Self {
        self.source = Some(path.display().to_string());
        self
    }

    pub fn fingerprint(&mut self, fingerprint: &str, coverage: &str) -> &mut Self {
        self.fingerprint = Some((fingerprint.to_string(), coverage.to_string()));
        self
    }

    pub fn part(&mut self, id: &str, name: &str, media: &str, description: &str) -> &mut Self {
        self.parts.push(Part {
            id: id.to_string(),
            name: name.to_string(),
            media: media.to_string(),
            description: description.to_string(),
        });
        self
    }

    pub fn action(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        results: &[String],
    ) -> &mut Self {
        self.actions.push(Action {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            results: results.to_vec(),
        });
        self
    }

    pub fn json(&self) -> Value {
        let mut graph = Vec::with_capacity(self.parts.len() + self.actions.len() + 4);
        graph.push(json!({
            "@id": METADATA,
            "@type": "CreativeWork",
            "about": {"@id": "./"},
            "conformsTo": {"@id": PROFILE},
            "description": "RO-Crate metadata that records the analysis of the dataset",
        }));
        let contents = self
            .parts
            .iter()
            .map(|part| json!({"@id": part.id}))
            .collect::<Vec<_>>();
        let mut mentions = vec![json!({"@id": "#software"})];
        if self.source.is_some() {
            mentions.push(json!({"@id": "#source"}));
        }
        let mut root = json!({
            "@id": "./",
            "@type": "Dataset",
            "name": self.name,
            "description": self.description,
            "conformsTo": {"@id": PROFILE},
            "hasPart": contents,
            "mentions": mentions,
        });
        if let Some((fingerprint, coverage)) = &self.fingerprint {
            root["identifier"] = json!(format!("sha256:{fingerprint}"));
            root["description"] = json!(format!(
                "{} · input fingerprint over {coverage}",
                self.description
            ));
        }
        graph.push(root);
        graph.push(json!({
            "@id": "#software",
            "@type": "SoftwareApplication",
            "name": self.software,
            "softwareVersion": self.version,
            "url": {"@id": "https://github.com/formetaohy/Grana"},
        }));
        if let Some(source) = &self.source {
            let name = Path::new(source).file_name().map_or_else(
                || source.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            graph.push(json!({
                "@id": "#source",
                "@type": "File",
                "name": name,
                "description": "the dataset this analysis reads",
                "contentUrl": format!("file:///{}", source.replace('\\', "/")),
            }));
        }
        for part in &self.parts {
            graph.push(json!({
                "@id": part.id,
                "@type": "File",
                "name": part.name,
                "description": part.description,
                "encodingFormat": part.media,
            }));
        }
        for action in &self.actions {
            let object = if self.source.is_some() {
                "#source"
            } else {
                "./"
            };
            graph.push(json!({
                "@id": action.id,
                "@type": ["CreateAction", "Action"],
                "name": action.name,
                "description": action.description,
                "instrument": {"@id": "#software"},
                "object": {"@id": object},
                "result": action
                    .results
                    .iter()
                    .map(|result| json!({"@id": result}))
                    .collect::<Vec<_>>(),
                "actionStatus": {"@id": COMPLETED},
            }));
        }
        json!({"@context": CONTEXT, "@graph": graph})
    }

    pub fn write(&self, root: &Path) -> Result<PathBuf, Error> {
        std::fs::create_dir_all(root).map_err(|error| Error::Write {
            detail: format!("failed to create {}: {error}", root.display()),
        })?;
        let text = serde_json::to_string_pretty(&self.json()).expect("a crate serializes");
        let path = root.join(METADATA);
        std::fs::write(&path, format!("{text}\n")).map_err(|error| Error::Write {
            detail: format!("failed to write {}: {error}", path.display()),
        })?;
        Ok(path)
    }
}
