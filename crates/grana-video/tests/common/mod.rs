use std::path::{Path, PathBuf};

pub fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("grana-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&path).ok();
    std::fs::create_dir_all(&path).expect("the scratch folder is created");
    path
}

pub fn names(directory: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(directory)
        .expect("the folder reads")
        .map(|entry| {
            entry
                .expect("the entry reads")
                .file_name()
                .into_string()
                .expect("the name is text")
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}
