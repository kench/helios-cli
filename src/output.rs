//! What a command prints: the API's JSON with `--json`, a short summary for people otherwise.

use std::fmt::Write;

use serde_json::Value;

/// A command's result.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// `{"devices": [...]}`.
    Devices(Value),
    /// One device (GetDevice's output).
    Device(Value),
    /// `{"streams": [...]}` (ListStreams).
    Streams(Value),
    /// `{"stream": {...}}`: GetStream, or a change's result.
    Stream(Value),
    /// Plain JSON either way (`schema`, `--dry-run`).
    Json(Value),
}

impl Output {
    /// The text to print: one JSON document with `json`, a summary otherwise.
    #[must_use]
    pub fn render(&self, json: bool) -> String {
        match (self, json) {
            (
                Self::Devices(value)
                | Self::Device(value)
                | Self::Streams(value)
                | Self::Stream(value)
                | Self::Json(value),
                true,
            )
            | (Self::Json(value), false) => {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
            (Self::Devices(value), false) => devices(value),
            (Self::Device(value), false) => device(value),
            (Self::Streams(value), false) => streams(value),
            (Self::Stream(value), false) => stream(&value["stream"]),
        }
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

fn devices(value: &Value) -> String {
    let rows: Vec<[String; 6]> = value["devices"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|device| {
            [
                text(&device["deviceId"]),
                text(&device["thingName"]),
                text(&device["role"]),
                text(&device["status"]),
                match device["connected"].as_bool() {
                    Some(true) => "yes".into(),
                    Some(false) => "no".into(),
                    None => "-".into(),
                },
                text(&device["heliosVersion"]),
            ]
        })
        .collect();
    let header = ["DEVICE", "THING", "ROLE", "STATUS", "CONNECTED", "HELIOS"].map(str::to_owned);
    let mut widths = header.clone().map(|cell| cell.len());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&header).chain(&rows) {
        let line: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
    if rows.is_empty() {
        out.push_str("(no devices)\n");
    }
    out.trim_end().to_owned()
}

/// `{"camera": {"video": "usb:1-1"}}` as `camera usb:1-1`.
fn source(value: &Value) -> String {
    match value.as_object().and_then(|map| map.iter().next()) {
        Some((kind, details)) => {
            let detail = ["video", "key", "size"]
                .iter()
                .find_map(|key| details[*key].as_str())
                .unwrap_or_default();
            format!("{kind} {detail}").trim_end().to_owned()
        }
        None => "-".into(),
    }
}

fn table<const N: usize>(header: [&str; N], rows: &[[String; N]], empty: &str) -> String {
    let header = header.map(str::to_owned);
    let mut widths = header.clone().map(|cell| cell.len());
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.len());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(&header).chain(rows) {
        let line: Vec<String> = row
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect();
        let _ = writeln!(out, "{}", line.join("  ").trim_end());
    }
    if rows.is_empty() {
        let _ = writeln!(out, "({empty})");
    }
    out.trim_end().to_owned()
}

fn streams(value: &Value) -> String {
    let rows: Vec<[String; 6]> = value["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|stream| {
            [
                text(&stream["streamId"]),
                text(&stream["desired"]["state"]),
                text(&stream["reported"]["state"]),
                if stream["pending"] == true {
                    "yes"
                } else {
                    "no"
                }
                .into(),
                source(&stream["reported"]["source"]),
                text(&stream["reported"]["bitrateKbps"]),
            ]
        })
        .collect();
    table(
        ["STREAM", "DESIRED", "REPORTED", "PENDING", "SOURCE", "KBPS"],
        &rows,
        "no streams",
    )
}

fn stream(stream: &Value) -> String {
    let (desired, reported) = (&stream["desired"], &stream["reported"]);
    let mut out = String::new();
    let mut line = |label: &str, value: String| {
        let _ = writeln!(out, "{label:12} {value}");
    };
    line("Stream", text(&stream["streamId"]));
    line(
        "State",
        format!(
            "{} (desired {})",
            text(&reported["state"]),
            text(&desired["state"])
        ),
    );
    line(
        "Pending",
        if stream["pending"] == true {
            "yes"
        } else {
            "no"
        }
        .into(),
    );
    line("Source", source(&reported["source"]));
    line("Profile", text(&reported["profile"]));
    line("Destination", text(&reported["destination"]));
    line(
        "Bitrate",
        format!("{} kbit/s", text(&reported["bitrateKbps"])),
    );
    if let Some(error) = reported["lastError"].as_str() {
        line("Last error", error.to_owned());
    }
    for rejected in stream["rejected"].as_array().into_iter().flatten() {
        line(
            "Rejected",
            format!(
                "{}: {} ({})",
                text(&rejected["setting"]),
                text(&rejected["reason"]),
                text(&rejected["at"])
            ),
        );
    }
    out.trim_end().to_owned()
}

fn device(value: &Value) -> String {
    let device = &value["device"];
    let mut out = String::new();
    for (label, key) in [
        ("Device", "deviceId"),
        ("Thing", "thingName"),
        ("Role", "role"),
        ("Status", "status"),
        ("Connected", "connected"),
        ("Helios", "heliosVersion"),
    ] {
        let _ = writeln!(out, "{label:10} {}", text(&device[key]));
    }
    if let Some(modules) = value["modules"].as_object() {
        let list: Vec<String> = modules
            .iter()
            .map(|(name, state)| format!("{name} {}", text(state)))
            .collect();
        let _ = writeln!(out, "{:10} {}", "Modules", list.join(", "));
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_is_the_api_response() {
        let value = json!({"devices": [{"deviceId": "d-1"}]});
        let rendered = Output::Devices(value.clone()).render(true);
        assert_eq!(serde_json::from_str::<Value>(&rendered).unwrap(), value);
        assert_eq!(
            serde_json::from_str::<Value>(&Output::Json(json!({"a": 1})).render(false)).unwrap(),
            json!({"a": 1})
        );
    }

    #[test]
    fn people_get_a_table() {
        let value = json!({"devices": [
            {"deviceId": "d-01M80KB9C9", "thingName": "t-01M80KAX04", "role": "transcoder",
             "status": "active", "connected": true, "heliosVersion": "0.6.0"},
            {"deviceId": "d-2", "thingName": "t-2", "role": "scanner", "status": "active"},
        ]});
        let table = Output::Devices(value).render(false);
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[0].starts_with("DEVICE"), "{table}");
        assert!(
            lines[1].contains("t-01M80KAX04")
                && lines[1].contains("yes")
                && lines[1].contains("0.6.0")
        );
        assert!(lines[2].contains("scanner") && lines[2].ends_with('-'));
        let none = Output::Devices(json!({"devices": [
            {"deviceId": "d-3", "connected": false},
        ]}))
        .render(false);
        assert!(none.contains("no"), "{none}");
        assert!(
            Output::Devices(json!({"devices": []}))
                .render(false)
                .contains("(no devices)")
        );
    }

    #[test]
    fn people_get_stream_tables_and_summaries() {
        let stream = json!({
            "streamId": "main", "pending": true,
            "desired": {"state": "live"},
            "reported": {"state": "starting", "source": {"camera": {"video": "usb:1-1"}},
                         "profile": "hd", "destination": "srt://h:1", "bitrateKbps": 4500,
                         "lastError": "no signal"},
            "rejected": [{"setting": "source", "reason": "busy", "at": "2026-11-22T00:00:00Z"}],
        });
        let table = Output::Streams(json!({"streams": [stream, {"streamId": "b"}]})).render(false);
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[0].starts_with("STREAM"), "{table}");
        assert!(
            lines[1].contains("camera usb:1-1") && lines[1].contains("yes"),
            "{table}"
        );
        assert!(
            lines[2].starts_with('b') && lines[2].contains("no"),
            "{table}"
        );
        assert!(
            Output::Streams(json!({"streams": []}))
                .render(false)
                .contains("(no streams)")
        );
        let summary = Output::Stream(json!({"stream": stream})).render(false);
        for line in [
            "State        starting (desired live)",
            "Pending      yes",
            "Source       camera usb:1-1",
            "Bitrate      4500 kbit/s",
            "Last error   no signal",
            "Rejected     source: busy (2026-11-22T00:00:00Z)",
        ] {
            assert!(summary.contains(line), "{line}\n{summary}");
        }
    }

    #[test]
    fn people_get_a_summary_of_one_device() {
        let value = json!({
            "device": {"deviceId": "d-1", "thingName": "t-1", "role": "transcoder",
                       "status": "active", "heliosVersion": "0.6.0"},
            "modules": {"net": "running", "usb": "running"},
        });
        let summary = Output::Device(value).render(false);
        assert!(summary.contains("Thing      t-1"), "{summary}");
        assert!(summary.contains("Connected  -"));
        assert!(summary.contains("Modules    net running, usb running"));
        assert!(
            !Output::Device(json!({"device": {}}))
                .render(false)
                .contains("Modules")
        );
    }
}
