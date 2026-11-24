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

/// Stack thread utama Windows hanya 1 MB. Di build debug, membuat `SettingsWindow` saja sudah memakai sekitar 1 MB
/// (kode Rust hasil generate Slint tanpa optimasi), belum termasuk `main()` dan event loop yang masih ada di stack,
/// sehingga membuka Pengaturan (tray, tab, atau shortcut) berakhir STATUS_STACK_OVERFLOW (0xc00000fd).
/// Naikkan stack thread utama lewat linker; berlaku untuk build debug maupun release.
fn raise_main_thread_stack() {
    const STACK_BYTES: u32 = 16 * 1024 * 1024;
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let flag = match env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => format!("/STACK:{STACK_BYTES}"),
        Ok("gnu") => format!("-Wl,--stack,{STACK_BYTES}"),
        _ => return,
    };
    println!("cargo:rustc-link-arg-bins={flag}");
    println!("cargo:rustc-link-arg-examples={flag}");
}


/// Salin utility native dari paket lama ke direktori di samping executable Slint.
/// Hanya runtime asset yang portabel yang disalin; capabilities/gen/permissions milik Tauri.
fn copy_dir_recursive(source: &std::path::Path, destination: &std::path::Path) {
    if !source.is_dir() {
        panic!("Bundled utility directory not found: {}", source.display());
    }
    fs::create_dir_all(destination).expect("could not create bundled utility directory");
    for entry in fs::read_dir(source).expect("could not read bundled utility directory") {
        let entry = entry.expect("could not read bundled utility entry");
        let from = entry.path();
        let to = destination.join(entry.file_name());
        println!("cargo:rerun-if-changed={}", from.display());
        if from.is_dir() {
            copy_dir_recursive(&from, &to);
        } else if from.is_file() {
            fs::copy(&from, &to).unwrap_or_else(|error| {
                panic!("failed to copy {}: {error}", from.display())
            });
        }
    }
}

fn copy_bundle_utilities() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bundled = manifest_dir.join("bundled");
    println!("cargo:rerun-if-changed={}", bundled.display());

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let profile = env::var("PROFILE").expect("PROFILE not set");
    let profile_dir = out_dir
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(profile.as_str()))
        .unwrap_or_else(|| panic!("could not find Cargo {profile} directory from OUT_DIR={}", out_dir.display()));

    // ADB diletakkan di <exe>/adb agar android_adb::find dapat menemukannya.
    let adb_source = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        bundled.join("adb")
    } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        bundled.join("adb-linux")
    } else {
        PathBuf::new()
    };
    if adb_source.is_dir() {
        let adb_destination = profile_dir.join("adb");
        copy_dir_recursive(&adb_source, &adb_destination);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let adb = adb_destination.join("adb");
            if adb.is_file() {
                let mut permissions = fs::metadata(&adb).expect("could not read adb permissions").permissions();
                permissions.set_mode(0o755);
                fs::set_permissions(&adb, permissions).expect("could not make bundled adb executable");
            }
        }
    }

    // Ghostscript hanya dibundel untuk Windows; Linux memakai instalasi sistem (PATH).
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        copy_dir_recursive(&bundled.join("gs"), &profile_dir.join("gs"));
        copy_dir_recursive(&bundled.join("pdfium"), &profile_dir.join("pdfium"));
    } else if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        copy_dir_recursive(&bundled.join("pdfium-linux"), &profile_dir.join("pdfium"));
    }

    // Font Office/PDF dan scrcpy-server tetap tersedia sebagai resource runtime.
    copy_dir_recursive(&bundled.join("resources"), &profile_dir.join("resources"));
}

fn main() {
    raise_main_thread_stack();
    copy_greeting_resources();
    copy_bundle_utilities();
    slint_build::compile("ui/notch.slint").unwrap();
}
