use serde_json::json;

use super::*;
use crate::client::tests::{client, server};

const DEVICE: &str = "d-01M80KB9C9T0ZSPC64NZVY1QRA";

fn set(passphrase_file: Option<PathBuf>, dry_run: bool) -> Wifi {
    Wifi::Set {
        device: DEVICE.into(),
        ssid: "Venue Media".into(),
        open: passphrase_file.is_none(),
        passphrase_file,
        hidden: true,
        dry_run,
    }
}

#[test]
fn checks_ssids_and_passphrases() {
    assert!(check_ssid("Home").is_ok());
    assert!(check_ssid(&"é".repeat(16)).is_ok(), "32 bytes");
    assert!(check_ssid(&"é".repeat(17)).is_err(), "34 bytes");
    assert!(check_ssid("").is_err());
    for good in ["eight ch", &"x".repeat(63), &"aB09".repeat(16)] {
        assert!(check_passphrase(good).is_ok(), "{good}");
    }
    for bad in [
        "seven c",
        &"x".repeat(65),
        &"g".repeat(64),
        "caf\u{e9} pass",
    ] {
        let error = check_passphrase(bad).unwrap_err().to_string();
        assert!(!error.contains(bad), "never echoes it");
    }
}

#[tokio::test]
async fn reads_the_passphrase_from_a_file_and_never_prints_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pass");
    std::fs::write(&file, "two words ok\r\nignored\n").unwrap();
    assert_eq!(
        read_passphrase(&file).unwrap(),
        "two words ok",
        "spaces kept"
    );
    let Output::Json(printed) = wifi(None, &set(Some(file.clone()), true)).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(
        printed,
        json!({"dryRun": true, "request": {
            "method": "PUT",
            "path": format!("/v1/control/devices/{DEVICE}/wifi/networks"),
            "body": {"ssid": "Venue Media", "hidden": true, "passphrase": "***"},
        }})
    );
    std::fs::write(&file, "short\n").unwrap();
    assert_eq!(
        wifi(None, &set(Some(file), true))
            .await
            .unwrap_err()
            .exit_code(),
        2
    );
    let missing = set(Some(dir.path().join("missing")), true);
    assert_eq!(wifi(None, &missing).await.unwrap_err().exit_code(), 2);
    let Output::Json(open) = wifi(None, &set(None, true)).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(
        open["request"]["body"],
        json!({"ssid": "Venue Media", "hidden": true})
    );
}

#[tokio::test]
async fn sets_and_removes_networks() {
    let (endpoint, received) = server(vec![
        (
            200,
            vec![],
            json!({"network": {"ssid": "Venue Media", "security": "personal", "hidden": true}})
                .to_string(),
        ),
        (200, vec![], json!({"removed": true}).to_string()),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pass");
    std::fs::write(&file, "not-a-real-pass\n").unwrap();
    let client = client(&endpoint);
    let Output::Json(answer) = wifi(Some(&client), &set(Some(file), false)).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(answer["network"]["security"], "personal");
    let remove = Wifi::Remove {
        device: DEVICE.into(),
        ssid: "Venue Media".into(),
        dry_run: false,
    };
    let Output::Json(removed) = wifi(Some(&client), &remove).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(removed, json!({"removed": true}));
    let requests = received.lock().unwrap().clone();
    assert_eq!(requests[0].method, "PUT");
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).unwrap(),
        json!({"ssid": "Venue Media", "hidden": true, "passphrase": "not-a-real-pass"}),
        "the API gets the passphrase"
    );
    assert_eq!(requests[1].method, "DELETE");
    assert_eq!(
        requests[1].target,
        format!("/dev/v1/control/devices/{DEVICE}/wifi/networks?ssid=Venue+Media")
    );
    let dry = Wifi::Remove {
        device: DEVICE.into(),
        ssid: "Venue Media".into(),
        dry_run: true,
    };
    assert!(offline(&dry));
    let Output::Json(printed) = wifi(None, &dry).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(printed["request"]["query"]["ssid"], "Venue Media");
    assert!(!offline(&Wifi::List {
        device: DEVICE.into()
    }));
    assert!(
        wifi(
            None,
            &Wifi::List {
                device: DEVICE.into()
            }
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn lists_inputs_profiles_and_networks() {
    let (endpoint, received) = server(vec![
        (200, vec![], json!({"inputs": [{"test": {}}]}).to_string()),
        (
            200,
            vec![],
            json!({"profiles": [{"name": "hevc"}]}).to_string(),
        ),
        (200, vec![], json!({"networks": []}).to_string()),
    ])
    .await;
    let client = client(&endpoint);
    let listed = inputs(
        &client,
        &Inputs::List {
            device: DEVICE.into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(listed, Output::Inputs(json!({"inputs": [{"test": {}}]})));
    let listed = profiles(
        &client,
        &Profiles::List {
            device: DEVICE.into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        listed,
        Output::Profiles(json!({"profiles": [{"name": "hevc"}]}))
    );
    let listed = wifi(
        Some(&client),
        &Wifi::List {
            device: DEVICE.into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(listed, Output::Wifi(json!({"networks": []})));
    let targets: Vec<String> = received
        .lock()
        .unwrap()
        .iter()
        .map(|request| request.target.clone())
        .collect();
    assert_eq!(
        targets,
        ["inputs", "profiles", "wifi/networks"]
            .map(|suffix| format!("/dev/v1/control/devices/{DEVICE}/{suffix}"))
    );
    assert_eq!(kind(&json!("odd")).0, "-");
}
