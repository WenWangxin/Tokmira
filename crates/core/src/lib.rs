pub mod advice;
pub mod attribution;
pub mod claude;
pub mod claude_quota;
pub mod codex;
pub mod codex_app_server;
pub mod dashboard;
pub mod detect;
pub mod environment;
pub mod estimate;
pub mod forecast;
pub mod i18n;
pub mod model;
pub mod pricing;
pub mod quota_store;
pub mod recorder;
pub mod report;
pub mod session_open;
pub mod sessions;
pub mod settings;
pub mod share;
pub mod timeline;
pub mod update;
pub mod usage;

/// Returns the version of the shared core library.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_matches_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
