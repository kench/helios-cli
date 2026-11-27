use serde_json::json;

use super::*;
use crate::client::tests::{client, server};

async fn run_args(args: &[&str]) -> (u8, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = run(
        std::iter::once("heliosctl")
            .chain(args.iter().copied())
            .map(OsString::from),
        &mut out,
        &mut err,
    )
    .await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[tokio::test]
async fn usage_errors_exit_2_and_help_exits_0() {
    let (code, _, err) = run_args(&["devices", "frobnicate"]).await;
    assert_eq!(code, 2, "{err}");
    let (code, out, _) = run_args(&["--help"]).await;
    assert_eq!(code, 0);
    assert!(out.contains("exit code"), "{out}");
    let (code, out, _) = run_args(&["--version"]).await;
    assert_eq!(code, 0);
    assert!(out.starts_with("heliosctl "));
}

#[tokio::test]
async fn schema_prints_the_operations() {
    let (code, out, _) = run_args(&["schema"]).await;
    assert_eq!(code, 0);
    let document: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(document["info"]["title"], "Helios Control API");
    let (code, out, _) = run_args(&["schema", "SetWifiNetwork"]).await;
    assert_eq!(code, 0);
    let operation: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(operation["method"], "PUT");
    assert_eq!(
        operation["path"],
        "/v1/control/devices/{deviceId}/wifi/networks"
    );
    let (code, _, err) = run_args(&["--json", "schema", "Frobnicate"]).await;
    assert_eq!(code, 2);
    let error: Value = serde_json::from_str(&err).unwrap();
    assert_eq!(error["error"]["type"], "InvalidInput");
}

#[test]
fn the_bundled_schema_is_the_pinned_contracts() {
    // The OpenAPI file comes from the release the contract submodule is pinned to: same
    // service version, and every operation the model defines.
    let document: Value = serde_json::from_str(OPENAPI).unwrap();
    let model = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contract/model/control.smithy"
    ))
    .expect("the contract submodule (git submodule update --init)");
    let version = document["info"]["version"].as_str().unwrap();
    assert!(
        model.contains(&format!("version: \"{version}\"")),
        "{version}"
    );
    let operations: Vec<&str> = document["paths"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|methods| methods.as_object().unwrap().values())
        .filter_map(|operation| operation["operationId"].as_str())
        .collect();
    for operation in &operations {
        assert!(
            model.contains(&format!("operation {operation} ")),
            "{operation}"
        );
    }
    let defined = model.matches("\noperation ").count();
    assert_eq!(operations.len(), defined, "every operation in the model");
}

#[tokio::test]
async fn a_command_without_an_endpoint_is_invalid() {
    // No endpoint anywhere (unless this machine's config file has one).
    let has_file = config::default_path().is_some_and(|path| path.exists());
    if has_file || std::env::var_os("HELIOS_CONTROL_URL").is_some() {
        return;
    }
    let (code, _, err) = run_args(&["--json", "devices", "list"]).await;
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("HELIOS_CONTROL_URL"));
}

#[test]
fn encodes_path_segments() {
    assert_eq!(segment("d-01ABC"), "d-01ABC");
    assert_eq!(segment("Venue / Media"), "Venue%20%2F%20Media");
    assert_eq!(segment("caf\u{e9}"), "caf%C3%A9");
}

#[tokio::test]
async fn finds_a_device_by_thing_name_across_pages() {
    let (endpoint, received) = server(vec![
        (
            200,
            vec![],
            json!({"devices": [{"deviceId": "d-1", "thingName": "t-1"}], "nextToken": "page2"})
                .to_string(),
        ),
        (
            200,
            vec![],
            json!({"devices": [{"deviceId": "d-2", "thingName": "t-2"}]}).to_string(),
        ),
    ])
    .await;
    let client = client(&endpoint);
    assert_eq!(
        device_id(&client, "d-9").await.unwrap(),
        "d-9",
        "an ID as it is"
    );
    assert_eq!(device_id(&client, "t-2").await.unwrap(), "d-2");
    let targets: Vec<String> = received
        .lock()
        .unwrap()
        .iter()
        .map(|request| request.target.clone())
        .collect();
    assert_eq!(
        targets,
        [
            "/dev/v1/control/devices?maxResults=100",
            "/dev/v1/control/devices?maxResults=100&nextToken=page2",
        ]
    );
    let missing = device_id(&client, "t-9").await.unwrap_err();
    assert_eq!(missing.exit_code(), 3);
}

#[tokio::test]
async fn lists_and_gets_devices() {
    let (endpoint, received) = server(vec![
        (
            200,
            vec![],
            json!({"devices": [{"deviceId": "d-1", "thingName": "t-1"}]}).to_string(),
        ),
        (
            200,
            vec![],
            json!({"device": {"deviceId": "d-1"}, "modules": {"net": "running"}}).to_string(),
        ),
    ])
    .await;
    let client = client(&endpoint);
    let listed = devices(&client, &Devices::List).await.unwrap();
    assert_eq!(
        listed,
        output::Output::Devices(json!({"devices": [{"deviceId": "d-1", "thingName": "t-1"}]}))
    );
    let got = devices(
        &client,
        &Devices::Get {
            device: "d-1".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        got,
        output::Output::Device(
            json!({"device": {"deviceId": "d-1"}, "modules": {"net": "running"}})
        )
    );
    assert_eq!(
        received.lock().unwrap()[1].target,
        "/dev/v1/control/devices/d-1"
    );
}

#[test]
fn the_command_line_is_well_formed() {
    // Catches clashing argument names, such as a command's --profile hiding the global one.
    <Cli as clap::CommandFactory>::command().debug_assert();
}

#[tokio::test]
async fn a_dry_run_for_a_device_id_needs_no_api() {
    let (code, out, err) = run_args(&[
        "--json",
        "streams",
        "start",
        "d-01M80KB9C9T0ZSPC64NZVY1QRA",
        "main",
        "--dry-run",
        "--client-token",
        "t1",
    ])
    .await;
    assert_eq!(code, 0, "{err}");
    let printed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(printed["request"]["body"], json!({"clientToken": "t1"}));
    let (code, _, _) = run_args(&["streams", "update", "d-1", "main", "--dry-run"]).await;
    assert_eq!(code, 2, "nothing to change");
}

#[test]
fn secrets_are_shown_only_when_asked() {
    let hidden = Cli::try_parse_from(["heliosctl", "wifi", "list", "d-1"]).unwrap();
    assert!(!hidden.show_secrets);
    let shown =
        Cli::try_parse_from(["heliosctl", "wifi", "list", "d-1", "--show-secrets"]).unwrap();
    assert!(shown.show_secrets, "global: after the subcommand too");
}
