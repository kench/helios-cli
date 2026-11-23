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
    /// `{"inputs": [...], "reportedAt"}` (ListInputs).
    Inputs(Value),
    /// `{"profiles": [...]}` (ListProfiles).
    Profiles(Value),
    /// `{"networks": [...], "reportedAt"}` (ListWifiNetworks).
    Wifi(Value),
    /// GetDeviceLocation's output: `gpsd`, `fix`, `lastFix`, `satellites`, `receivers`...
    Location(Value),
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
                | Self::Inputs(value)
                | Self::Profiles(value)
                | Self::Wifi(value)
                | Self::Location(value)
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
            (Self::Inputs(value), false) => inputs(value),
            (Self::Profiles(value), false) => profiles(value),
            (Self::Wifi(value), false) => wifi(value),
            (Self::Location(value), false) => location(value),
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

fn rows<const N: usize>(
    value: &Value,
    key: &str,
    row: impl Fn(&Value) -> [String; N],
) -> Vec<[String; N]> {
    value[key]
        .as_array()
        .into_iter()
        .flatten()
        .map(row)
        .collect()
}

fn reported(out: String, value: &Value) -> String {
    match value["reportedAt"].as_str() {
        Some(at) => format!("{out}\n(reported {at})"),
        None => out,
    }
}

fn inputs(value: &Value) -> String {
    let rows = rows(value, "inputs", |input| {
        let (kind, details) = crate::inventory::kind(input);
        let what = details["selector"]
            .as_str()
            .or(details["key"].as_str())
            .unwrap_or_default()
            .to_owned();
        let about = [&details["manufacturer"], &details["product"]]
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let extra = match kind.as_str() {
            "camera" if details["hasAudio"] == true => "has audio".to_owned(),
            "rtmp" => details["publisher"]
                .as_object()
                .map_or("idle", |_| "publishing")
                .to_owned(),
            _ => String::new(),
        };
        [kind, what, text(&details["port"]), about, extra]
    });
    reported(
        table(
            ["KIND", "SELECTOR", "PORT", "DEVICE", "NOTE"],
            &rows,
            "no inputs",
        ),
        value,
    )
}

fn profiles(value: &Value) -> String {
    let rows = rows(value, "profiles", |profile| {
        [
            text(&profile["name"]),
            text(&profile["video"]),
            text(&profile["encoder"]),
            text(&profile["bitrateMode"]),
            text(&profile["bitrateKbps"]),
            format!(
                "{}-{}",
                text(&profile["minBitrateKbps"]),
                text(&profile["maxBitrateKbps"])
            ),
            text(&profile["audio"]),
        ]
    });
    table(
        [
            "PROFILE", "VIDEO", "ENCODER", "MODE", "KBPS", "RANGE", "AUDIO",
        ],
        &rows,
        "no profiles",
    )
}

fn wifi(value: &Value) -> String {
    let rows = rows(value, "networks", |network| {
        [
            text(&network["ssid"]),
            text(&network["security"]),
            if network["hidden"] == true {
                "yes"
            } else {
                "no"
            }
            .into(),
        ]
    });
    reported(
        table(["SSID", "SECURITY", "HIDDEN"], &rows, "no networks"),
        value,
    )
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

fn location(value: &Value) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{:10} {}", "gpsd", text(&value["gpsd"]));
    let fix = &value["fix"];
    if fix["mode"] == "none" || fix["mode"].is_null() {
        let _ = writeln!(out, "{:10} none since {}", "Fix", text(&value["since"]));
        if value["lastFix"].is_object() {
            fix_lines(&mut out, "Last fix", &value["lastFix"]);
        }
    } else {
        fix_lines(&mut out, "Fix", fix);
    }
    let satellites = &value["satellites"];
    if satellites.is_object() {
        let mut line = format!(
            "{} used of {} visible",
            text(&satellites["used"]),
            text(&satellites["visible"])
        );
        if let Some(hdop) = satellites["hdop"].as_f64() {
            let _ = write!(line, ", HDOP {hdop}");
        }
        let _ = writeln!(out, "{:10} {line}", "Satellites");
    }
    let receivers: Vec<String> = value["receivers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|receiver| {
            let mut about: Vec<String> = Vec::new();
            if let Some(driver) = receiver["driver"].as_str() {
                about.push(driver.to_owned());
            }
            if let Some(port) = receiver["usbPort"].as_str() {
                about.push(format!("USB {port}"));
            }
            if about.is_empty() {
                text(&receiver["path"])
            } else {
                format!("{} ({})", text(&receiver["path"]), about.join(", "))
            }
        })
        .collect();
    let _ = writeln!(
        out,
        "{:10} {}",
        "Receivers",
        if receivers.is_empty() {
            "none".to_owned()
        } else {
            receivers.join("; ")
        }
    );
    let configured = &value["configuredReceiver"];
    if configured["added"] == false {
        let _ = writeln!(
            out,
            "{:10} {} not open in gpsd: {}",
            "Configured",
            text(&configured["path"]),
            text(&configured["error"])
        );
    }
    reported(out.trim_end().to_owned(), value)
}

/// A fix's lines: mode and time, then the position, altitude and motion it has.
fn fix_lines(out: &mut String, label: &str, fix: &Value) {
    let _ = writeln!(
        out,
        "{label:10} {} at {}",
        text(&fix["mode"]),
        text(&fix["time"])
    );
    if let (Some(latitude), Some(longitude)) = (fix["latitude"].as_f64(), fix["longitude"].as_f64())
    {
        let mut line = format!("{latitude}, {longitude}");
        if let Some(error) = fix["horizontalErrorM"].as_f64() {
            let _ = write!(line, " (±{error} m)");
        }
        let _ = writeln!(out, "{:10} {line}", "Position");
    }
    if let Some(altitude) = fix["altitudeM"].as_f64() {
        let _ = writeln!(out, "{:10} {altitude} m", "Altitude");
    }
    if let Some(speed) = fix["speedMps"].as_f64() {
        let mut line = format!("{speed} m/s");
        if let Some(track) = fix["trackDeg"].as_f64() {
            let _ = write!(line, ", track {track}°");
        }
        let _ = writeln!(out, "{:10} {line}", "Speed");
    }
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
    fn summarizes_a_location() {
        // Made-up coordinates.
        let current = Output::Location(json!({
            "gpsd": "connected",
            "receivers": [{"path": "/dev/ttyACM0", "driver": "NMEA0183", "usbPort": "3-1.2"}],
            "fix": {"mode": "3d", "time": "2026-11-22T20:15:30Z", "latitude": 12.345678,
                    "longitude": -98.765432, "altitudeM": 41.5, "speedMps": 0.12,
                    "trackDeg": 270.0, "horizontalErrorM": 4.2},
            "satellites": {"used": 9, "visible": 14, "hdop": 0.9},
            "since": "2026-11-22T20:10:02Z",
            "configuredReceiver": {"path": "/dev/ttyACM0", "added": true},
            "reportedAt": "2026-11-22T20:15:30Z",
        }))
        .render(false);
        assert_eq!(
            current,
            "gpsd       connected\n\
             Fix        3d at 2026-11-22T20:15:30Z\n\
             Position   12.345678, -98.765432 (±4.2 m)\n\
             Altitude   41.5 m\n\
             Speed      0.12 m/s, track 270°\n\
             Satellites 9 used of 14 visible, HDOP 0.9\n\
             Receivers  /dev/ttyACM0 (NMEA0183, USB 3-1.2)\n\
             (reported 2026-11-22T20:15:30Z)"
        );
        let lost = Output::Location(json!({
            "gpsd": "unavailable",
            "receivers": [{"path": "/dev/ttyUSB0"}],
            "fix": {"mode": "none"},
            "lastFix": {"mode": "2d", "time": "2026-11-22T19:00:00Z", "latitude": 12.3,
                        "longitude": -98.7},
            "since": "2026-11-22T19:00:05Z",
            "configuredReceiver": {"path": "/dev/ttyACM0", "added": false, "error": "gpsd: ERROR"},
        }))
        .render(false);
        assert_eq!(
            lost,
            "gpsd       unavailable\n\
             Fix        none since 2026-11-22T19:00:05Z\n\
             Last fix   2d at 2026-11-22T19:00:00Z\n\
             Position   12.3, -98.7\n\
             Receivers  /dev/ttyUSB0\n\
             Configured /dev/ttyACM0 not open in gpsd: gpsd: ERROR"
        );
        let nothing = Output::Location(json!({"gpsd": "connected", "fix": {"mode": "none"},
                                              "receivers": [], "since": "2026-11-22T19:00:05Z"}))
        .render(false);
        assert!(nothing.ends_with("Receivers  none"), "{nothing}");
        let json = Output::Location(json!({"fix": {"mode": "none"}})).render(true);
        assert!(json.contains("\"mode\": \"none\""), "{json}");
    }

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
    fn people_get_inputs_profiles_and_networks() {
        let inputs = Output::Inputs(json!({"inputs": [
            {"camera": {"selector": "0bda:5830:CAM1", "port": "usb:1-1", "manufacturer": "Acme",
                        "product": "Cam", "hasAudio": true}},
            {"audio": {"selector": "hw:1", "port": "usb:1-2"}},
            {"rtmp": {"key": "ingest", "publisher": {"address": "192.0.2.1"}}},
            {"rtmp": {"key": "ingest-2"}},
            {"test": {}},
        ], "reportedAt": "2026-11-22T00:00:00Z"}))
        .render(false);
        let lines: Vec<&str> = inputs.lines().collect();
        assert!(
            lines[1].contains("0bda:5830:CAM1") && lines[1].contains("Acme Cam"),
            "{inputs}"
        );
        assert!(lines[1].ends_with("has audio"), "{inputs}");
        assert!(
            lines[3].ends_with("publishing") && lines[4].ends_with("idle"),
            "{inputs}"
        );
        assert!(inputs.ends_with("(reported 2026-11-22T00:00:00Z)"));
        let profiles =
            Output::Profiles(json!({"profiles": [{"name": "hevc", "video": "hevc_rkmpp",
            "encoder": "belacoder", "bitrateMode": "adaptive", "bitrateKbps": 6000,
            "minBitrateKbps": 1000, "maxBitrateKbps": 8000, "audio": "copy"}]}))
            .render(false);
        assert!(
            profiles.contains("1000-8000") && profiles.contains("belacoder"),
            "{profiles}"
        );
        let networks = Output::Wifi(json!({"networks": [
            {"ssid": "Home", "security": "personal", "hidden": false},
            {"ssid": "Back", "security": "open", "hidden": true},
        ]}))
        .render(false);
        assert!(
            networks.lines().nth(2).unwrap().ends_with("yes"),
            "{networks}"
        );
        assert!(
            Output::Wifi(json!({"networks": []}))
                .render(false)
                .contains("(no networks)")
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
