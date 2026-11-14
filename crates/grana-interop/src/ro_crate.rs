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
    size: Option<u64>,
    sha256: Option<String>,
}

pub struct Input {
    id: String,
    name: String,
    description: String,
    location: Option<String>,
    sha256: Option<String>,
    size: Option<u64>,
}

pub struct Action {
    id: String,
    name: String,
    description: String,
    object: Option<String>,
    results: Vec<String>,
}

pub struct Crate {
    name: String,
    description: String,
    software: String,
    version: String,
    author: Option<String>,
    date: Option<String>,
    source: Option<String>,
    fingerprint: Option<(String, String)>,
    inputs: Vec<Input>,
    collections: Vec<Input>,
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
            author: None,
            date: None,
            source: None,
            fingerprint: None,
            inputs: Vec::new(),
            collections: Vec::new(),
            parts: Vec::new(),
            actions: Vec::new(),
        }
    }

    pub fn author(&mut self, name: &str) -> &mut Self {
        self.author = Some(name.to_string());
        self
    }

    pub fn date(&mut self, iso: &str) -> &mut Self {
        self.date = Some(iso.to_string());
        self
    }

    pub fn source(&mut self, path: &Path) -> &mut Self {
        self.source = Some(path.display().to_string());
        self
    }

    pub fn fingerprint(&mut self, fingerprint: &str, coverage: &str) -> &mut Self {
        self.fingerprint = Some((fingerprint.to_string(), coverage.to_string()));
        self
    }

    pub fn input(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        location: Option<&Path>,
        sha256: Option<&str>,
        size: Option<u64>,
    ) -> &mut Self {
        self.inputs.push(Input {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            location: location.map(|path| path.display().to_string()),
            sha256: sha256.map(str::to_string),
            size,
        });
        self
    }

    pub fn collection(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        location: Option<&Path>,
        size: Option<u64>,
    ) -> &mut Self {
        self.collections.push(Input {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            location: location.map(|path| path.display().to_string()),
            sha256: None,
            size,
        });
        self
    }

    pub fn part(&mut self, id: &str, name: &str, media: &str, description: &str) -> &mut Self {
        self.parts.push(Part {
            id: id.to_string(),
            name: name.to_string(),
            media: media.to_string(),
            description: description.to_string(),
            size: None,
            sha256: None,
        });
        self
    }

    pub fn hashed_part(
        &mut self,
        id: &str,
        name: &str,
        media: &str,
        description: &str,
        size: Option<u64>,
        sha256: Option<&str>,
    ) -> &mut Self {
        self.parts.push(Part {
            id: id.to_string(),
            name: name.to_string(),
            media: media.to_string(),
            description: description.to_string(),
            size,
            sha256: sha256.map(str::to_string),
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
            object: None,
            results: results.to_vec(),
        });
        self
    }

    pub fn action_on(
        &mut self,
        id: &str,
        name: &str,
        description: &str,
        object: &str,
        results: &[String],
    ) -> &mut Self {
        self.actions.push(Action {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            object: Some(object.to_string()),
            results: results.to_vec(),
        });
        self
    }

    pub fn json(&self) -> Value {
        let mut graph =
            Vec::with_capacity(self.parts.len() + self.actions.len() + self.inputs.len() + 5);
        graph.push(json!({
            "@id": METADATA,
            "@type": "CreativeWork",
            "about": {"@id": "./"},
            "conformsTo": {"@id": PROFILE},
            "description": "RO-Crate metadata that records the analysis of the dataset",
        }));
        let mut contents = self
            .parts
            .iter()
            .map(|part| json!({"@id": part.id}))
            .collect::<Vec<_>>();
        for collection in &self.collections {
            contents.push(json!({"@id": collection.id}));
        }
        let mut mentions = vec![json!({"@id": "#software"})];
        if self.source.is_some() {
            mentions.push(json!({"@id": "#source"}));
        }
        for input in &self.inputs {
            mentions.push(json!({"@id": input.id}));
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
        if let Some(date) = &self.date {
            root["datePublished"] = json!(date);
        }
        if let Some(author) = &self.author {
            root["author"] = json!({"@id": "#author"});
            graph.push(json!({
                "@id": "#author",
                "@type": "Person",
                "name": author,
            }));
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
        for input in &self.inputs {
            let mut entity = json!({
                "@id": input.id,
                "@type": "Dataset",
                "name": input.name,
                "description": input.description,
            });
            if let Some(location) = &input.location {
                entity["contentUrl"] = json!(format!("file:///{}", location.replace('\\', "/")));
            }
            if let Some(sha256) = &input.sha256 {
                entity["identifier"] = json!(format!("sha256:{sha256}"));
            }
            if let Some(size) = input.size {
                entity["contentSize"] = json!(size.to_string());
            }
            graph.push(entity);
        }
        for collection in &self.collections {
            let mut entity = json!({
                "@id": collection.id,
                "@type": "Dataset",
                "name": collection.name,
                "description": collection.description,
            });
            if let Some(location) = &collection.location {
                entity["contentUrl"] = json!(format!("file:///{}", location.replace('\\', "/")));
            }
            if let Some(size) = collection.size {
                entity["contentSize"] = json!(size.to_string());
            }
            graph.push(entity);
        }
        for part in &self.parts {
            let mut entity = json!({
                "@id": part.id,
                "@type": "File",
                "name": part.name,
                "description": part.description,
                "encodingFormat": part.media,
            });
            if let Some(size) = part.size {
                entity["contentSize"] = json!(size.to_string());
            }
            if let Some(sha256) = &part.sha256 {
                entity["sha256"] = json!(sha256);
            }
            graph.push(entity);
        }
        for action in &self.actions {
            let object = action.object.clone().unwrap_or_else(|| {
                if self.source.is_some() {
                    "#source".to_string()
                } else {
                    "./".to_string()
                }
            });
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
        grana_file::write(&path, format!("{text}\n").as_bytes()).map_err(|error| Error::Write {
            detail: format!("failed to write {}: {error}", path.display()),
        })?;
        Ok(path)
    }
}
