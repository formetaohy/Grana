#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    pub name: String,
    pub extensions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    File { filters: Vec<Filter> },
    Folder,
    Save { name: String, filters: Vec<Filter> },
}

impl Kind {
    pub fn filters(&self) -> &[Filter] {
        match self {
            Self::File { filters } | Self::Save { filters, .. } => filters,
            Self::Folder => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inquiry {
    title: String,
    kind: Kind,
}

impl Inquiry {
    fn new(title: impl Into<String>, kind: Kind) -> Self {
        let title = title.into();
        assert!(
            !title.trim().is_empty(),
            "an inquiry names itself in its title bar"
        );
        Self { title, kind }
    }

    pub fn choose(title: impl Into<String>) -> Self {
        Self::new(
            title,
            Kind::File {
                filters: Vec::new(),
            },
        )
    }

    pub fn folder(title: impl Into<String>) -> Self {
        Self::new(title, Kind::Folder)
    }

    pub fn save(title: impl Into<String>, name: impl Into<String>) -> Self {
        let name = name.into();
        assert!(
            !name.trim().is_empty(),
            "a save inquiry suggests a file name"
        );
        Self::new(
            title,
            Kind::Save {
                name,
                filters: Vec::new(),
            },
        )
    }

    pub fn filter(mut self, name: &str, extensions: &[&str]) -> Self {
        let filter = Filter {
            name: name.to_string(),
            extensions: extensions
                .iter()
                .map(|extension| (*extension).to_string())
                .collect(),
        };
        assert!(
            !filter.extensions.is_empty(),
            "an inquiry filter admits at least one extension"
        );
        match &mut self.kind {
            Kind::File { filters } | Kind::Save { filters, .. } => filters.push(filter),
            Kind::Folder => panic!("a folder inquiry filters no extension"),
        }
        self
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn mode(&self) -> &Kind {
        &self.kind
    }
}
