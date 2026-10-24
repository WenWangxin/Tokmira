use std::path::{Path, PathBuf};

fn main() {
    let target = env_var("TARGET");
    let manifest_dir = PathBuf::from(env_var("CARGO_MANIFEST_DIR"));
    let sidecar = sidecar_path(&manifest_dir, &target);
    println!("cargo:rerun-if-changed={}", sidecar.display());

    // `tauri-build` copies `bundle.externalBin` beside the desktop executable and
    // fails when the file is missing. Release (`pnpm tauri build`) must have it.
    // Debug (`cargo build` / `clippy` / `test`, and `pnpm tauri dev`) drops the
    // key so those commands do not depend on a packaged sidecar. Dev still finds
    // `target/debug/token-audit` from `cargo build -p token-audit-cli`, which
    // sits next to `token-audit-desktop`.
    let release = env_var("PROFILE") == "release";
    if release && !sidecar.is_file() {
        eprintln!(
            "error: missing sidecar {}\n\
             Build it with: sh scripts/build-sidecar.sh {target}\n\
             On Windows: .\\scripts\\build-sidecar.ps1 -Target {target}",
            sidecar.display()
        );
        std::process::exit(1);
    }
    if !release {
        strip_external_bin();
    }

    tauri_build::build();
}

fn sidecar_path(manifest_dir: &Path, target: &str) -> PathBuf {
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    manifest_dir
        .join("binaries")
        .join(format!("token-audit-{target}{extension}"))
}

fn strip_external_bin() {
    // JSON Merge Patch (RFC 7396): null removes the key when `tauri-build` merges
    // `TAURI_CONFIG` over `tauri.conf.json`. Other bundle fields stay.
    let mut overlay = match std::env::var("TAURI_CONFIG") {
        Ok(raw) => serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };
    if let Some(bundle) = overlay
        .get_mut("bundle")
        .and_then(serde_json::Value::as_object_mut)
    {
        bundle.insert("externalBin".to_owned(), serde_json::Value::Null);
    } else if let Some(root) = overlay.as_object_mut() {
        root.insert(
            "bundle".to_owned(),
            serde_json::json!({ "externalBin": null }),
        );
    }
    // SAFETY: a build script runs on one thread, and this is the only `set_var`.
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var("TAURI_CONFIG", overlay.to_string());
    }
}

fn env_var(name: &str) -> String {
    match std::env::var(name) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("error: environment variable {name} is not set");
            std::process::exit(1);
        }
    }
}
