//! Secrets in what heliosctl prints: a destination's passphrase (in its SRT/SRTLA URL) and a
//! Wi-Fi network's passphrase. The Control API returns both to operators (owner, 2026-11-26),
//! but heliosctl shows them as `***` unless `--show-secrets` is given, so they stay out of
//! terminals, logs and agents' transcripts by default.

use serde_json::Value;

/// `value` with every secret in it replaced by `***`: each `passphrase` member that's a string,
/// and the `passphrase` parameter of each `srt://` or `srtla://` URL.
#[must_use]
pub fn hide(value: Value) -> Value {
    match value {
        Value::String(text) if text.starts_with("srt://") || text.starts_with("srtla://") => {
            Value::String(hide_in_url(&text))
        }
        Value::Array(items) => Value::Array(items.into_iter().map(hide).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| match value {
                    Value::String(_) if key == "passphrase" => (key, Value::String("***".into())),
                    other => (key, hide(other)),
                })
                .collect(),
        ),
        other => other,
    }
}

fn hide_in_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_owned();
    };
    let pairs: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some(("passphrase", _)) => "passphrase=***".to_owned(),
            _ => pair.to_owned(),
        })
        .collect();
    format!("{base}?{}", pairs.join("&"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::hide;

    #[test]
    fn hides_every_passphrase() {
        let shown = json!({
            "stream": {
                "desired": {"destination": "srtla://h:5000?streamid=a&passphrase=a-passphrase-1&latency=2000"},
                "reported": {"destination": "srt://h:1?passphrase=x2345678901"},
            },
            "networks": [{"ssid": "Home", "passphrase": "correct horse battery"}, {"ssid": "Cafe"}],
            "network": {"ssid": "Home", "passphrase": "correct horse battery"},
        });
        let hidden = hide(shown);
        assert_eq!(
            hidden["stream"]["desired"]["destination"],
            "srtla://h:5000?streamid=a&passphrase=***&latency=2000"
        );
        assert_eq!(
            hidden["stream"]["reported"]["destination"],
            "srt://h:1?passphrase=***"
        );
        assert_eq!(hidden["networks"][0]["passphrase"], "***");
        assert_eq!(
            hidden["networks"][1].get("passphrase"),
            None,
            "none stays none"
        );
        assert_eq!(hidden["network"]["passphrase"], "***");
        assert!(!hidden.to_string().contains("correct horse"));
        // Not a URL with a query, or not a string: as it was.
        assert_eq!(hide(json!("srt://h:1")), json!("srt://h:1"));
        assert_eq!(
            hide(json!({"passphrase": null})),
            json!({"passphrase": null})
        );
    }
}
