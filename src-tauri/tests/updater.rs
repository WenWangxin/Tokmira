//! The update scenarios from M13's acceptance list, against a local HTTP server and a
//! test-only key pair (not the release key). Only checking and downloading run here;
//! installing replaces the running app and is verified by hand.

// A separate test binary on purpose: building a mock Tauri app stops macOS file events
// from reaching watchers created later in the same process, which broke the M9 watcher test.
#[path = "../src/update_text.rs"]
mod update_text;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;

use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri_plugin_updater::{Updater, UpdaterExt};
use update_text::{info, message};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/updater");
    std::fs::read_to_string(path.join(name)).unwrap()
}

/// Serves `routes` (path → body) until the test process exits; unknown paths get 404.
fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                line.clear();
            }
            let (status, body) = routes
                .iter()
                .find(|(route, _)| *route == path)
                .map_or(("404 Not Found", &b""[..]), |(_, body)| {
                    ("200 OK", body.as_slice())
                });
            let mut stream = &stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        }
    });
    format!("http://{address}")
}

fn manifest(base: &str, version: &str, signature: &str) -> Vec<u8> {
    let platform = json!({ "url": format!("{base}/payload.bin"), "signature": signature });
    let platforms: serde_json::Map<_, _> = [
        "darwin-aarch64",
        "darwin-x86_64",
        "windows-x86_64",
        "windows-aarch64",
        "linux-x86_64",
    ]
    .into_iter()
    .map(|target| (target.to_owned(), platform.clone()))
    .collect();
    serde_json::to_vec(&json!({
        "version": version,
        "notes": "  修复若干问题\n",
        "pub_date": "2026-10-27T00:00:00Z",
        "platforms": platforms,
    }))
    .unwrap()
}

fn updater(endpoint: &str, require_signed_version: bool) -> Updater {
    let mut context = mock_context::<MockRuntime, _>(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".to_owned(),
        json!({
            "pubkey": fixture("test.key.pub"),
            "endpoints": [endpoint],
            "dangerousInsecureTransportProtocol": true,
            "requireSignedVersion": require_signed_version,
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    app.updater().unwrap()
}

/// Serves a manifest announcing `version` and the test payload, then checks.
fn check(
    version: &str,
    signature: &str,
    payload: &[u8],
    require_signed_version: bool,
) -> tauri_plugin_updater::Result<Option<tauri_plugin_updater::Update>> {
    // The manifest needs the server's address, so it is served from a second server.
    let files = serve(vec![("/payload.bin", payload.to_vec())]);
    let base = serve(vec![("/latest.json", manifest(&files, version, signature))]);
    tauri::async_runtime::block_on(
        updater(&format!("{base}/latest.json"), require_signed_version).check(),
    )
}

fn download(update: &tauri_plugin_updater::Update) -> tauri_plugin_updater::Result<Vec<u8>> {
    tauri::async_runtime::block_on(update.download(|_, _| {}, || {}))
}

#[test]
fn a_newer_version_is_found_and_its_signed_package_downloads() {
    let payload = fixture("payload.bin");
    let found = check(
        "0.2.0",
        &fixture("payload.bin.sig"),
        payload.as_bytes(),
        false,
    )
    .unwrap()
    .expect("0.2.0 is newer than the mock app's 0.1.0");
    let shown = info(&found);
    assert_eq!(shown.version, "0.2.0");
    assert_eq!(shown.notes.as_deref(), Some("修复若干问题"));
    assert_eq!(download(&found).unwrap(), payload.as_bytes());
}

#[test]
fn the_same_version_is_not_an_update() {
    let found = check("0.1.0", &fixture("payload.bin.sig"), b"", false).unwrap();
    assert!(found.is_none());
}

#[test]
fn a_package_that_does_not_match_its_signature_is_rejected() {
    let found = check("0.2.0", &fixture("payload.bin.sig"), b"tampered", false)
        .unwrap()
        .unwrap();
    let error = download(&found).unwrap_err();
    assert_eq!(
        message(&error, token_audit_core::i18n::Lang::Zh),
        "安装包签名校验失败，已停止安装"
    );
}

#[test]
fn a_signature_for_another_version_is_rejected() {
    let payload = fixture("payload.bin");
    // Signed for 0.2.0 but announced as 0.3.0: a replayed old package.
    let found = check(
        "0.3.0",
        &fixture("payload-0.2.0.bin.sig"),
        payload.as_bytes(),
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        message(
            &download(&found).unwrap_err(),
            token_audit_core::i18n::Lang::Zh
        ),
        "安装包签名校验失败，已停止安装"
    );
    let found = check(
        "0.2.0",
        &fixture("payload-0.2.0.bin.sig"),
        payload.as_bytes(),
        true,
    )
    .unwrap()
    .unwrap();
    assert_eq!(download(&found).unwrap(), payload.as_bytes());
}

#[test]
fn a_malformed_manifest_is_reported() {
    let base = serve(vec![("/latest.json", b"{\"version\": 1}".to_vec())]);
    let error =
        tauri::async_runtime::block_on(updater(&format!("{base}/latest.json"), false).check())
            .err()
            .unwrap();
    assert_eq!(
        message(&error, token_audit_core::i18n::Lang::Zh),
        "更新清单格式不正确或缺少本机平台的安装包"
    );
}

#[test]
fn an_unreachable_server_is_a_network_error() {
    // Bind and drop to get a local port with nothing listening.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let error = tauri::async_runtime::block_on(
        updater(&format!("http://127.0.0.1:{port}/latest.json"), false).check(),
    )
    .err()
    .unwrap();
    assert_eq!(
        message(&error, token_audit_core::i18n::Lang::Zh),
        "无法连接 GitHub，请检查网络后重试"
    );
}
