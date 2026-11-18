use std::env;
use std::fs;
use std::path::PathBuf;

fn copy_greeting_resources() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_dir = manifest_dir
        .parent()
        .expect("workspace root not found")
        .join("public")
        .join("greeting");

    println!("cargo:rerun-if-changed={}", source_dir.display());
    if !source_dir.is_dir() {
        panic!("Greeting resources not found: {}", source_dir.display());
    }

    // Find target/<profile> by its name instead of relying on a fixed number of
    // parent directories: Cargo may add crate/hash directory levels to OUT_DIR.
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let profile = env::var("PROFILE").expect("PROFILE not set");
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile.as_str()))
        .unwrap_or_else(|| panic!("could not find Cargo {profile} directory from OUT_DIR={}", out_dir.display()));
    let destination_dir = profile_dir.join("greeting");

    if destination_dir.exists() {
        fs::remove_dir_all(&destination_dir)
            .expect("could not clear previous greeting resources");
    }
    fs::create_dir_all(&destination_dir)
        .expect("could not create greeting resource directory");

    let entries = fs::read_dir(&source_dir).expect("could not read public/greeting");
    for entry in entries {
        let entry = entry.expect("could not read greeting resource entry");
        let source = entry.path();
        if !source.is_file() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", source.display());
        let name = entry.file_name();
        fs::copy(&source, destination_dir.join(name))
            .unwrap_or_else(|error| panic!("failed to copy {}: {error}", source.display()));
    }
}

fn main() {
    copy_greeting_resources();
    slint_build::compile("ui/notch.slint").unwrap();
}
