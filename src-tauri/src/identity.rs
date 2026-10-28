//! Display branding is independent of the identities used by released installations.

pub const DISPLAY_NAME: &str = "Tokmira";
pub const LEGACY_NAME: &str = "token-audit";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebranding_preserves_the_released_installation_contract() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("Tauri config");
        assert_eq!(config["productName"], DISPLAY_NAME);
        assert_eq!(config["app"]["windows"][0]["title"], DISPLAY_NAME);
        assert_eq!(LEGACY_NAME, "token-audit");
        assert_eq!(config["identifier"], "app.tokenaudit.desktop");
        assert_eq!(config["mainBinaryName"], "token-audit-desktop");
        assert_eq!(config["bundle"]["externalBin"][0], "binaries/token-audit");
        assert_eq!(
            config["plugins"]["updater"]["endpoints"][0],
            "https://github.com/WenWangxin/token-audit/releases/latest/download/latest.json"
        );
        assert_eq!(config["plugins"]["updater"]["requireSignedVersion"], true);
        assert_eq!(
            config["bundle"]["windows"]["nsis"]["template"],
            "windows/installer.nsi"
        );
        // Windows must detect the old installation before choosing an install path.
        let installer = include_str!("../windows/installer.nsi");
        let package: serde_json::Value =
            serde_json::from_str(include_str!("../../package.json")).expect("root package");
        assert_eq!(package["devDependencies"]["@tauri-apps/cli"], "2.12.4");
        assert!(installer.contains("!define INSTALLIDENTITY \"token-audit\""));
        assert!(installer.contains("!define UNINSTKEY \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\${INSTALLIDENTITY}\""));
        assert!(installer.contains("!define MANUPRODUCTKEY \"${MANUKEY}\\${INSTALLIDENTITY}\""));
        assert!(installer
            .contains("WriteRegStr SHCTX \"${UNINSTKEY}\" \"DisplayName\" \"${PRODUCTNAME}\""));
    }
}
