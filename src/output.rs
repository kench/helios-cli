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
    /// Plain JSON either way (`schema`).
    Json(Value),
}

impl Output {
    /// The text to print: one JSON document with `json`, a summary otherwise.
    #[must_use]
    pub fn render(&self, json: bool) -> String {
        match (self, json) {
            (Self::Devices(value) | Self::Device(value) | Self::Json(value), true)
            | (Self::Json(value), false) => {
                serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
            }
            (Self::Devices(value), false) => devices(value),
            (Self::Device(value), false) => device(value),
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
