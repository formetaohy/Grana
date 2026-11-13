use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=installer/windows/grana.rc");
    println!("cargo:rerun-if-changed=installer/windows/grana.manifest");
    println!("cargo:rerun-if-changed=installer/windows/art/grana.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").expect("the package version is defined");
    let icon = absolute(Path::new("installer/windows/art/grana.ico"));
    let manifest = absolute(Path::new("installer/windows/grana.manifest"));
    let template = std::fs::read_to_string("installer/windows/grana.rc")
        .expect("the windows resource template is readable");
    let script = template
        .replace("@VERSION@", &version)
        .replace("@VERSION_COMMA@", &version_comma(&version))
        .replace("@ICON@", &icon)
        .replace("@MANIFEST@", &manifest);

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("the output directory is defined"));
    let resource = out.join("grana.rc");
    std::fs::write(&resource, script).expect("the windows resource script is writable");
    embed_resource::compile(&resource, embed_resource::NONE)
        .manifest_required()
        .expect("the windows resource compiles");
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .expect("the windows resource input exists")
        .display()
        .to_string()
        .replace('\\', "/")
}

fn version_comma(version: &str) -> String {
    let mut parts = version.split('.').map(|part| {
        part.parse::<u16>()
            .expect("the package version is numeric")
            .to_string()
    });
    let major = parts.next().expect("the package version has a major field");
    let minor = parts.next().unwrap_or_else(|| "0".to_string());
    let patch = parts.next().unwrap_or_else(|| "0".to_string());
    format!("{major},{minor},{patch}")
}
