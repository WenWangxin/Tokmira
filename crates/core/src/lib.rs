pub mod claude;
pub mod claude_quota;
pub mod codex;
pub mod codex_app_server;
pub mod model;
pub mod pricing;
pub mod recorder;
pub mod report;

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
