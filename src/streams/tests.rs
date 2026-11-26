use serde_json::json;

use super::*;
use crate::client::tests::{client, server};

const DEVICE: &str = "d-01M80KB9C9T0ZSPC64NZVY1QRA";

fn target(wait: bool, timeout: u64) -> Target {
    Target {
        device: DEVICE.into(),
        stream: "main".into(),
        dry_run: false,
        wait,
        timeout,
        client_token: Some("token-1".into()),
    }
}

fn described(pending: bool, rejected: Value) -> String {
    json!({"stream": {
        "streamId": "main",
        "desired": {"state": "live"},
        "reported": {"state": if pending { "stopped" } else { "live" },
                     "destination": "srt://h:1?streamid=a&passphrase=not-a-secret-123"},
        "pending": pending,
        "rejected": rejected,
    }, "shadowVersion": 7})
    .to_string()
}

#[test]
fn reads_sources() {
    for (text, value) in [
        (
            "camera:usb:1-1.3",
            json!({"camera": {"video": "usb:1-1.3"}}),
        ),
        (
            "camera:0bda:5830:CAM1,audio=hw:1",
            json!({"camera": {"video": "0bda:5830:CAM1", "audio": "hw:1"}}),
        ),
        ("rtmp:ingest", json!({"rtmp": {"key": "ingest"}})),
        ("rtmp:ingest-2", json!({"rtmp": {"key": "ingest-2"}})),
        ("test", json!({"test": {}})),
        (
            "test:1920x1080@30",
            json!({"test": {"size": "1920x1080", "fps": 30}}),
        ),
        ("test@60", json!({"test": {"fps": 60}})),
    ] {
        assert_eq!(parse_source(text).unwrap(), value, "{text}");
    }
    for bad in [
        "camera:usb:",
        "camera:usb:1",
        "camera:zzzz:5830:CAM1",
        "camera:0bda:5830:",
        "camera:usb:1-1,audio=",
        "rtmp:live",
        "rtmp:ingest-0",
        "rtmp:ingest-1000",
        "test:1920",
        "test@0",
        "test@121",
        "file:/x",
    ] {
        assert!(parse_source(bad).is_err(), "{bad}");
    }
}

#[test]
fn reads_bitrates_and_names() {
    assert_eq!(
        parse_bitrate("default").unwrap(),
        json!({"profileDefault": {}})
    );
    assert_eq!(
        parse_bitrate("4500").unwrap(),
        json!({"overrideKbps": 4500})
    );
    for bad in ["99", "100001", "fast", "-1"] {
        assert!(parse_bitrate(bad).is_err(), "{bad}");
    }
    assert!(check_name("stream", "main_2-b").is_ok());
    for bad in ["", "a b", "a/b", &"x".repeat(65)] {
        assert!(check_name("stream", bad).is_err(), "{bad}");
    }
}

#[test]
fn never_shows_an_srt_passphrase() {
    let value = json!({"streams": [{"reported": {
        "destination": "srt://h:1?streamid=a&passphrase=not-a-secret-123&latency=200",
        "lastError": "srt://not a url, left alone",
    }}], "previous": "srtla://h:2?passphrase=other-secret-1", "n": 3});
    assert_eq!(
        redact(value),
        json!({"streams": [{"reported": {
            "destination": "srt://h:1?streamid=a&passphrase=***&latency=200",
            "lastError": "srt://not a url, left alone",
        }}], "previous": "srtla://h:2?passphrase=***", "n": 3})
    );
    assert_eq!(redact(json!("srt://h:1")), json!("srt://h:1"));
}

#[tokio::test]
async fn a_dry_run_checks_and_prints_without_sending() {
    let command = Streams::SetDestination {
        target: Target {
            dry_run: true,
            ..target(false, 60)
        },
        url: Some("srt://ingest.example.com:4001?latency=2000&streamid=publish/live/a".into()),
        url_file: None,
    };
    let Output::Json(printed) = run(None, &command).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(
        printed,
        json!({"dryRun": true, "request": {
            "method": "PUT",
            "path": format!("/v1/control/devices/{DEVICE}/streams/main/destination"),
            "body": {"url": "srt://ingest.example.com:4001?streamid=publish/live/a&latency=2000",
                     "clientToken": "token-1"},
        }})
    );
    let bad = Streams::SetDestination {
        target: Target {
            dry_run: true,
            ..target(false, 60)
        },
        url: Some("srt://host:1?mode=caller".into()),
        url_file: None,
    };
    let failure = run(None, &bad).await.unwrap_err();
    assert_eq!(failure.exit_code(), 2);
    assert_eq!(failure.to_json()["error"]["part"], "mode");
}

#[tokio::test]
async fn a_passphrase_never_comes_from_the_command_line() {
    let command = Streams::SetDestination {
        target: target(false, 60),
        url: Some("srt://h:1?passphrase=not-a-secret-123".into()),
        url_file: None,
    };
    let failure = run(None, &command).await.unwrap_err();
    assert!(matches!(failure, Failure::Invalid(ref m) if m.contains("--url-file")));

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("url");
    std::fs::write(&file, "srt://h:1?passphrase=not-a-secret-123\nignored\n").unwrap();
    let from_file = Streams::SetDestination {
        target: Target {
            dry_run: true,
            ..target(false, 60)
        },
        url: None,
        url_file: Some(file),
    };
    let Output::Json(printed) = run(None, &from_file).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(
        printed["request"]["body"]["url"], "srt://h:1?passphrase=***",
        "checked, sent, and redacted when printed"
    );
    std::fs::write(dir.path().join("empty"), "\n").unwrap();
    for path in [dir.path().join("empty"), dir.path().join("missing")] {
        let command = Streams::SetDestination {
            target: target(false, 60),
            url: None,
            url_file: Some(path),
        };
        assert_eq!(run(None, &command).await.unwrap_err().exit_code(), 2);
    }
}

#[tokio::test]
async fn update_needs_something_valid_to_change() {
    let update =
        |source: Option<&str>, bitrate: Option<&str>, percent: Option<u16>| Streams::Update {
            target: Target {
                dry_run: true,
                ..target(false, 60)
            },
            source: source.map(str::to_owned),
            encoding_profile: None,
            bitrate: bitrate.map(str::to_owned),
            source_ceiling_percent: percent,
            record: None,
        };
    assert_eq!(
        run(None, &update(None, None, None))
            .await
            .unwrap_err()
            .exit_code(),
        2
    );
    assert_eq!(
        run(None, &update(None, None, Some(401)))
            .await
            .unwrap_err()
            .exit_code(),
        2
    );
    let Output::Json(printed) = run(
        None,
        &update(Some("rtmp:ingest"), Some("default"), Some(150)),
    )
    .await
    .unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(printed["request"]["method"], "PATCH");
    assert_eq!(
        printed["request"]["body"],
        json!({"source": {"rtmp": {"key": "ingest"}}, "bitrate": {"profileDefault": {}},
               "sourceCeilingPercent": 150, "clientToken": "token-1"})
    );
    let profiled = Streams::Update {
        target: Target {
            dry_run: true,
            client_token: None,
            ..target(false, 60)
        },
        source: None,
        encoding_profile: Some("hd-30".into()),
        bitrate: None,
        source_ceiling_percent: None,
        record: Some("off".into()),
    };
    let Output::Json(printed) = run(None, &profiled).await.unwrap() else {
        panic!("not JSON");
    };
    assert_eq!(printed["request"]["body"]["profile"], "hd-30");
    assert_eq!(printed["request"]["body"]["record"], false);
    assert_eq!(
        printed["request"]["body"]["clientToken"]
            .as_str()
            .unwrap()
            .len(),
        32,
        "a random token"
    );
    let bad_token = Streams::Start(Target {
        client_token: Some("no spaces".into()),
        ..target(false, 60)
    });
    assert_eq!(run(None, &bad_token).await.unwrap_err().exit_code(), 2);
}

#[tokio::test]
async fn waits_until_the_device_applies_the_change() {
    let (endpoint, received) = server(vec![
        (202, vec![], described(true, json!([]))),
        (200, vec![], described(false, json!([]))),
    ])
    .await;
    let client = client(&endpoint);
    let Output::Stream(done) = run(Some(&client), &Streams::Start(target(true, 10)))
        .await
        .unwrap()
    else {
        panic!("not a stream");
    };
    assert_eq!(done["stream"]["pending"], false);
    assert_eq!(
        done["stream"]["reported"]["destination"],
        "srt://h:1?streamid=a&passphrase=***"
    );
    let requests = received.lock().unwrap().clone();
    assert_eq!(requests[0].method, "POST");
    assert_eq!(
        requests[0].target,
        format!("/dev/v1/control/devices/{DEVICE}/streams/main/start")
    );
    assert_eq!(
        serde_json::from_str::<Value>(&requests[0].body).unwrap(),
        json!({"clientToken": "token-1"})
    );
    assert_eq!(requests[1].method, "GET");
}

#[tokio::test]
async fn a_new_rejection_is_a_refusal() {
    let old = json!([{"setting": "profile", "reason": "old", "at": "2026-11-21T00:00:00Z"}]);
    let new = json!([
        {"setting": "profile", "reason": "old", "at": "2026-11-21T00:00:00Z"},
        {"setting": "source", "reason": "no such camera", "at": "2026-11-22T00:00:00Z"},
    ]);
    let (endpoint, _) = server(vec![
        (202, vec![], described(true, old)),
        (200, vec![], described(false, new)),
    ])
    .await;
    let failure = run(Some(&client(&endpoint)), &Streams::Stop(target(true, 10)))
        .await
        .unwrap_err();
    assert_eq!(failure.exit_code(), 4);
    assert_eq!(
        failure.to_string(),
        "the device refused source: no such camera"
    );
}

#[tokio::test]
async fn waiting_gives_up_after_the_timeout() {
    let (endpoint, _) = server(vec![(202, vec![], described(true, json!([])))]).await;
    let started = Instant::now();
    let failure = run(Some(&client(&endpoint)), &Streams::Start(target(true, 1)))
        .await
        .unwrap_err();
    assert_eq!(failure.exit_code(), 5);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn lists_gets_and_changes_without_waiting() {
    let (endpoint, received) = server(vec![
        (
            200,
            vec![],
            json!({"streams": [{"streamId": "main", "pending": false}]}).to_string(),
        ),
        (200, vec![], described(false, json!([]))),
        (202, vec![], described(true, json!([]))),
    ])
    .await;
    let client = client(&endpoint);
    assert!(matches!(
        run(
            Some(&client),
            &Streams::List {
                device: DEVICE.into()
            }
        )
        .await
        .unwrap(),
        Output::Streams(_)
    ));
    let get = Streams::Get {
        device: DEVICE.into(),
        stream: "main".into(),
    };
    assert!(matches!(
        run(Some(&client), &get).await.unwrap(),
        Output::Stream(_)
    ));
    let Output::Stream(changed) = run(Some(&client), &Streams::Stop(target(false, 60)))
        .await
        .unwrap()
    else {
        panic!("not a stream");
    };
    assert_eq!(changed["stream"]["pending"], true, "returned at once");
    let targets: Vec<String> = received
        .lock()
        .unwrap()
        .iter()
        .map(|request| request.target.clone())
        .collect();
    assert_eq!(
        targets,
        [
            format!("/dev/v1/control/devices/{DEVICE}/streams"),
            format!("/dev/v1/control/devices/{DEVICE}/streams/main"),
            format!("/dev/v1/control/devices/{DEVICE}/streams/main/stop"),
        ]
    );
    let bad = Streams::Get {
        device: DEVICE.into(),
        stream: "a/b".into(),
    };
    assert_eq!(run(Some(&client), &bad).await.unwrap_err().exit_code(), 2);
    assert!(
        run(
            None,
            &Streams::List {
                device: DEVICE.into()
            }
        )
        .await
        .is_err()
    );
}
