//! The `heliosctl` binary end to end: settings from the environment, credentials from AWS files
//! through the credential chain, a signed request to a local server, and the exit code.
#![allow(
    clippy::unwrap_used,
    reason = "test helpers, outside #[test] functions"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;

/// Answers one request with `status` and `body` on a local port; sends what it got.
fn serve_once(
    status: u16,
    error_type: Option<&'static str>,
    body: &'static str,
) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            head.push_str(&line);
        }
        let mut body_in = vec![0u8; length];
        let _ = reader.read_exact(&mut body_in);
        tx.send(head).unwrap();
        let mut stream = stream;
        let extra = error_type.map_or(String::new(), |kind| {
            format!("x-amzn-errortype: {kind}\r\n")
        });
        let _ = write!(
            stream,
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (format!("http://{address}/dev"), rx)
}

/// `heliosctl` with throwaway AWS files holding static keys for profile `helios-operator`.
fn heliosctl(endpoint: &str, args: &[&str]) -> (i32, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let credentials = dir.path().join("credentials");
    std::fs::write(
        &credentials,
        "[helios-operator]\naws_access_key_id = AKIDEXAMPLE\naws_secret_access_key = secret\n",
    )
    .unwrap();
    let config = dir.path().join("config");
    std::fs::write(&config, "[profile helios-operator]\nregion = us-west-2\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_heliosctl"))
        .args(args)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir.path())
        .env("XDG_CONFIG_HOME", dir.path())
        .env("AWS_SHARED_CREDENTIALS_FILE", &credentials)
        .env("AWS_CONFIG_FILE", &config)
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .env("HELIOS_CONTROL_URL", endpoint)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

#[test]
fn lists_devices_with_signed_requests() {
    let (endpoint, received) = serve_once(
        200,
        None,
        r#"{"devices": [{"deviceId": "d-1", "thingName": "t-1", "role": "transcoder", "status": "active"}]}"#,
    );
    let (code, out, err) = heliosctl(&endpoint, &["--json", "devices", "list"]);
    assert_eq!(code, 0, "{err}");
    let document: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(document["devices"][0]["thingName"], "t-1");
    let head = received.recv().unwrap();
    assert!(
        head.starts_with("GET /dev/v1/control/devices?maxResults=100 "),
        "{head}"
    );
    assert!(
        head.to_ascii_lowercase()
            .contains("authorization: aws4-hmac-sha256 credential=akidexample/"),
        "{head}"
    );
}

#[test]
fn reports_the_apis_error_and_its_exit_code() {
    let (endpoint, _received) = serve_once(
        404,
        Some("DeviceNotFound"),
        r#"{"message": "No device d-9."}"#,
    );
    let (code, out, err) = heliosctl(&endpoint, &["--json", "devices", "get", "d-9"]);
    assert_eq!(code, 3, "{err}");
    assert!(out.is_empty());
    let error: serde_json::Value = serde_json::from_str(&err).unwrap();
    assert_eq!(error["error"]["type"], "DeviceNotFound");
    assert_eq!(error["error"]["message"], "No device d-9.");
}

#[test]
fn people_get_a_message_on_stderr() {
    let (code, out, err) = heliosctl("http://insecure.example.com/dev", &["devices", "list"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.starts_with("heliosctl: endpoint"), "{err}");
}

#[test]
fn asks_for_public_addresses_only_with_the_flag() {
    let body = r#"{"uplinks": [{"interface": "eth0", "kind": "ethernet", "operState": "up", "ipv4": [], "sourceRouted": true, "bonding": []}]}"#;
    let (endpoint, received) = serve_once(200, None, body);
    let (code, out, err) = heliosctl(&endpoint, &["--json", "uplinks", "d-1"]);
    assert_eq!(code, 0, "{err}");
    let document: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(document["uplinks"][0]["interface"], "eth0");
    let head = received.recv().unwrap();
    assert!(
        head.starts_with("GET /dev/v1/control/devices/d-1/uplinks "),
        "{head}"
    );

    let (endpoint, received) = serve_once(200, None, body);
    let (code, _, err) = heliosctl(
        &endpoint,
        &["--json", "uplinks", "d-1", "--show-public-addresses"],
    );
    assert_eq!(code, 0, "{err}");
    let head = received.recv().unwrap();
    assert!(
        head.starts_with("GET /dev/v1/control/devices/d-1/uplinks?includePublicAddresses=true "),
        "{head}"
    );
}
