use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res/*");

    let config_path = std::path::Path::new("res/wisp.toml");

    if !config_path.exists() {
        panic!("Config file not found: {:?}, create it by copying wisp.toml.example to wisp.toml and editing it", config_path);
    }

    // Hard-coded to point to the workspace target
    let target_dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .join("target")
        .join(std::env::var("PROFILE").unwrap());

    // Copy the config file to the target directory
    std::fs::copy(config_path, target_dir.join("wisp.toml")).expect("Failed to copy config file");
}
