use std::{env, fs, path::PathBuf};

fn main() {
    // fluent-dark styles the stock scroll bar to sit on the app's dark theme.
    let config = slint_build::CompilerConfiguration::new().with_style("fluent-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("compile UI");
    embed_resources();
}

/// Writes app.rc with the package version filled in and embeds it.
fn embed_resources() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=icon.ico");
    // CI stamps the release version here just before building.
    println!("cargo:rerun-if-changed=Cargo.toml");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    // FILEVERSION takes four numbers; drop any pre-release suffix ("1.2.3-beta").
    let mut parts: Vec<&str> = version.split(['.', '-', '+']).take(3).collect();
    parts.resize(3, "0");
    let commas = format!("{},0", parts.join(","));

    let icon = manifest_dir.join("icon.ico").display().to_string().replace('\\', "\\\\");
    let rc = fs::read_to_string(manifest_dir.join("app.rc"))
        .expect("read app.rc")
        .replace("@ICON@", &icon)
        .replace("@VERSION_COMMAS@", &commas)
        .replace("@VERSION@", &version);

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("app.rc");
    fs::write(&out, rc).expect("write app.rc");
    embed_resource::compile(&out, embed_resource::NONE).manifest_required().expect("embed resources");
}
