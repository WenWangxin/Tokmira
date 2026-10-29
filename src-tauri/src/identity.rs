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
            "https://github.com/WenWangxin/Tokmira/releases/latest/download/latest.json"
        );
        assert_eq!(config["plugins"]["updater"]["requireSignedVersion"], true);
        assert_eq!(
            config["bundle"]["windows"]["nsis"]["template"],
            "windows/installer.nsi"
        );
        // The old repository URL redirects to Tokmira for released updater clients.
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

        // Limit the text search to the autostart branch so update mode cannot
        // accidentally remove either legacy registry value.
        let autostart = installer
            .split_once("; Removes the Autostart entry")
            .expect("autostart uninstall section")
            .1
            .split_once("; Delete app data if the checkbox is selected")
            .expect("end of autostart uninstall section")
            .0;
        let guarded = autostart
            .split_once("${If} $UpdateMode <> 1")
            .expect("non-update guard")
            .1
            .split_once("${EndIf}")
            .expect("end of non-update guard")
            .0;
        assert!(guarded.contains(
            "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Run\" \"${PRODUCTNAME}\""
        ));
        assert!(guarded.contains(
            "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Run\" \"${INSTALLIDENTITY}\""
        ));
        assert!(guarded.contains(
            "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run\" \"${INSTALLIDENTITY}\""
        ));
        let autostart_builder = include_str!("main.rs")
            .split_once("tauri_plugin_autostart::Builder::new()")
            .expect("autostart builder")
            .1
            .split_once(".build()")
            .expect("end of autostart builder")
            .0;
        assert!(autostart_builder.contains(".app_name(identity::LEGACY_NAME)"));
    }
}
