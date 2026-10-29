//! One-time macOS bundle rename. All paths passed to the testable executor are explicit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use token_audit_core::recorder::{self, RecorderParts};

const OLD_NAME: &str = "token-audit.app";
const NEW_NAME: &str = "Tokmira.app";
const BUNDLE_ID: &str = "app.tokenaudit.desktop";
const MARKER: &str = "bundle-rename-autostart";

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Migrate,
    Skip(&'static str),
}

pub fn decide(
    bundle: &Path,
    identifier: &str,
    packaged: bool,
    destination_exists: bool,
    parent_writable: bool,
) -> Decision {
    if !cfg!(target_os = "macos") || !packaged {
        return Decision::Skip("not a packaged macOS app");
    }
    if identifier != BUNDLE_ID {
        return Decision::Skip("unexpected bundle identifier");
    }
    if bundle.file_name().and_then(|name| name.to_str()) != Some(OLD_NAME) {
        return Decision::Skip("bundle has another name");
    }
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Decision::Skip("translocated app");
    }
    if destination_exists {
        return Decision::Skip("destination already exists");
    }
    if !parent_writable {
        return Decision::Skip("parent directory is not writable");
    }
    Decision::Migrate
}

fn target(bundle: &Path) -> io::Result<PathBuf> {
    Ok(bundle
        .parent()
        .ok_or_else(|| io::Error::other("bundle has no parent"))?
        .join(NEW_NAME))
}

pub fn marker(data_dir: &Path) -> PathBuf {
    data_dir.join(MARKER)
}

fn new_cli(bundle: &Path) -> PathBuf {
    bundle.join("Contents/MacOS/token-audit")
}

fn restore_recorder(home: &Path, data_dir: &Path, bundle: &Path, parts: RecorderParts) {
    if parts.statusline || parts.hooks {
        let _ = recorder::install(home, data_dir, &new_cli(bundle), parts);
    }
}

/// Returns true only when the renamed app has been launched. `launch` is injected
/// so tests never start an application or contact the real login item service.
pub fn execute(
    bundle: &Path,
    home: &Path,
    data_dir: &Path,
    parts: RecorderParts,
    autostart: bool,
    launch: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<bool> {
    let destination = target(bundle)?;
    // Check again immediately before rename; never replace another installation.
    if fs::symlink_metadata(&destination).is_ok() {
        return Ok(false);
    }
    if autostart {
        fs::create_dir_all(data_dir)?;
        fs::write(marker(data_dir), b"enabled\n")?;
    }
    if let Err(error) = fs::rename(bundle, &destination) {
        if autostart {
            let _ = fs::remove_file(marker(data_dir));
        }
        return Err(error);
    }
    if parts.statusline || parts.hooks {
        if let Err(error) = recorder::install(home, data_dir, &new_cli(&destination), parts) {
            restore_recorder(home, data_dir, bundle, parts);
            let _ = fs::rename(&destination, bundle);
            if autostart {
                let _ = fs::remove_file(marker(data_dir));
            }
            return Err(io::Error::other(error));
        }
    }
    if let Err(error) = launch(&destination) {
        restore_recorder(home, data_dir, bundle, parts);
        let _ = fs::rename(&destination, bundle);
        if autostart {
            let _ = fs::remove_file(marker(data_dir));
        }
        return Err(error);
    }
    Ok(true)
}

pub fn should_migrate(bundle: &Path, identifier: &str, packaged: bool) -> Decision {
    let Ok(destination) = target(bundle) else {
        return Decision::Skip("bundle has no parent");
    };
    let destination_exists = fs::symlink_metadata(destination).is_ok();
    let Some(parent) = bundle.parent() else {
        return Decision::Skip("bundle has no parent");
    };
    use objc2_foundation::{NSFileManager, NSString};
    let parent_writable = NSFileManager::defaultManager()
        .isWritableFileAtPath(&NSString::from_str(&parent.to_string_lossy()));
    // Writable mounted volumes are allowed; read-only DMGs are rejected by the
    // parent check and, ultimately, by the atomic rename itself.
    decide(
        bundle,
        identifier,
        packaged,
        destination_exists,
        parent_writable,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tokmira-bundle-rename-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("test directory");
            Self(path)
        }
        fn bundle(&self) -> PathBuf {
            self.0.join(OLD_NAME)
        }
        fn make_bundle(&self) {
            fs::create_dir(self.bundle()).expect("fake bundle");
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    #[test]
    fn migration_decision_checks_each_skip_condition() {
        let old = Path::new("/tmp/token-audit.app");
        assert_eq!(
            decide(old, BUNDLE_ID, false, false, true),
            Decision::Skip("not a packaged macOS app")
        );
        assert_eq!(
            decide(old, "other.app", true, false, true),
            Decision::Skip("unexpected bundle identifier")
        );
        assert_eq!(
            decide(Path::new("/tmp/Custom.app"), BUNDLE_ID, true, false, true),
            Decision::Skip("bundle has another name")
        );
        assert_eq!(
            decide(
                Path::new("/tmp/AppTranslocation/x/token-audit.app"),
                BUNDLE_ID,
                true,
                false,
                true
            ),
            Decision::Skip("translocated app")
        );
        assert_eq!(
            decide(old, BUNDLE_ID, true, true, true),
            Decision::Skip("destination already exists")
        );
        assert_eq!(
            decide(old, BUNDLE_ID, true, false, false),
            Decision::Skip("parent directory is not writable")
        );
        assert_eq!(
            decide(
                Path::new("/Volumes/ReadOnly/token-audit.app"),
                BUNDLE_ID,
                true,
                false,
                false
            ),
            Decision::Skip("parent directory is not writable")
        );
        assert_eq!(decide(old, BUNDLE_ID, true, false, true), Decision::Migrate);
    }

    #[test]
    fn executor_renames_and_uses_new_bundle_without_running_open() {
        let temp = Temp::new();
        temp.make_bundle();
        let new = temp.0.join(NEW_NAME);
        let launched = execute(
            &temp.bundle(),
            &temp.0.join("home"),
            &temp.0.join("data"),
            RecorderParts::default(),
            false,
            |bundle| {
                assert_eq!(bundle, new);
                Ok(())
            },
        )
        .expect("rename");
        assert!(launched);
        assert!(!temp.bundle().exists());
        assert!(new.is_dir());
    }

    #[test]
    fn executor_skips_existing_target() {
        let temp = Temp::new();
        temp.make_bundle();
        fs::create_dir(temp.0.join(NEW_NAME)).expect("existing target");
        assert!(!execute(
            &temp.bundle(),
            &temp.0.join("home"),
            &temp.0.join("data"),
            RecorderParts::default(),
            false,
            |_| panic!("must not launch")
        )
        .expect("skip"));
        assert!(temp.bundle().is_dir());
    }

    #[test]
    fn executor_restores_bundle_when_recorder_fails() {
        let temp = Temp::new();
        temp.make_bundle();
        let home = temp.0.join("home");
        fs::create_dir_all(home.join(".claude")).expect("claude directory");
        fs::write(home.join(".claude/settings.json"), b"invalid json").expect("bad settings");
        let result = execute(
            &temp.bundle(),
            &home,
            &temp.0.join("data"),
            RecorderParts {
                statusline: true,
                hooks: false,
            },
            false,
            |_| panic!("must not launch"),
        );
        assert!(result.is_err());
        assert!(temp.bundle().is_dir());
        assert!(!temp.0.join(NEW_NAME).exists());
    }

    #[test]
    fn executor_restores_bundle_when_launch_fails() {
        let temp = Temp::new();
        temp.make_bundle();
        let result = execute(
            &temp.bundle(),
            &temp.0.join("home"),
            &temp.0.join("data"),
            RecorderParts::default(),
            true,
            |_| Err(io::Error::other("synthetic launch failure")),
        );
        assert!(result.is_err());
        assert!(temp.bundle().is_dir());
        assert!(!temp.0.join(NEW_NAME).exists());
        assert!(!marker(&temp.0.join("data")).exists());
    }

    #[test]
    fn executor_points_recorder_at_renamed_bundle() {
        let temp = Temp::new();
        temp.make_bundle();
        let home = temp.0.join("home");
        let data = temp.0.join("data");
        fs::create_dir_all(home.join(".claude")).expect("settings directory");
        fs::write(
            home.join(".claude/settings.json"),
            include_str!("../../crates/core/tests/fixtures/recorder/bundle-rename-settings.json"),
        )
        .expect("settings fixture");
        let parts = RecorderParts {
            statusline: true,
            hooks: false,
        };
        recorder::install(&home, &data, &new_cli(&temp.bundle()), parts).expect("old recorder");
        execute(&temp.bundle(), &home, &data, parts, true, |_| Ok(())).expect("rename");
        assert!(temp.0.join(NEW_NAME).is_dir());
        assert_eq!(fs::read(marker(&data)).expect("marker"), b"enabled\n");
        let settings: serde_json::Value = serde_json::from_slice(
            &fs::read(home.join(".claude/settings.json")).expect("settings"),
        )
        .expect("settings json");
        let command = settings["statusLine"]["command"].as_str().expect("command");
        assert!(command.contains(NEW_NAME) && !command.contains(OLD_NAME));
        let status = recorder::status(&home, &data);
        assert!(status.statusline_installed && !status.statusline_modified);
    }
}
