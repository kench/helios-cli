//! `heliosctl streams`: list, get, start, stop, update and set-destination (HLD 7.3.4.1).
//!
//! Changes carry a client token, so they're retried safely; `--dry-run` checks the input with
//! the service's rules and prints the request without sending it; `--wait` polls GetStream until
//! the device has applied the change (exit 0), refused it (4) or the time runs out (5).

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};
use serde_json::{Value, json};

use crate::cli::{device_id, segment};
use crate::client::{Client, Method};
use crate::destination;
use crate::error::Failure;
use crate::output::Output;

/// `heliosctl streams`.
#[derive(Debug, Subcommand)]
pub enum Streams {
    /// A device's streams: desired and reported state.
    List {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
    },
    /// One stream.
    Get {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
        /// The stream's name.
        stream: String,
    },
    /// Start a stream (desired state live).
    Start(Target),
    /// Stop a stream (desired state stopped).
    Stop(Target),
    /// Change a stream's source, profile, bitrate, source ceiling or recording.
    Update {
        /// The stream, and how to make the change.
        #[command(flatten)]
        target: Target,
        /// camera:<video>[,audio=<device>], rtmp:<key>, or test[:<WxH>][@<fps>].
        #[arg(long)]
        source: Option<String>,
        /// The encoding profile's name (--profile is the AWS profile).
        #[arg(long, value_name = "NAME")]
        encoding_profile: Option<String>,
        /// A bitrate override in kbit/s (100–100000), or `default` for the profile's.
        #[arg(long)]
        bitrate: Option<String>,
        /// Adaptive RTMP streams: the ceiling as a percentage of the camera's bitrate (0–400).
        #[arg(long)]
        source_ceiling_percent: Option<u16>,
        /// Record the stream on the device as sent (on) or not (off). Switching restarts the
        /// stream's encoder: a few seconds' gap.
        #[arg(long, value_parser = ["on", "off"])]
        record: Option<String>,
    },
    /// Set where a stream goes: an srt:// or srtla:// URL.
    ///
    /// A URL with an SRT passphrase must come from a file or stdin (--url-file -), never the
    /// command line.
    SetDestination {
        /// The stream, and how to make the change.
        #[command(flatten)]
        target: Target,
        /// The URL, if it has no passphrase.
        #[arg(
            long,
            conflicts_with = "url_file",
            required_unless_present = "url_file"
        )]
        url: Option<String>,
        /// A file holding the URL, or - for stdin.
        #[arg(long)]
        url_file: Option<PathBuf>,
    },
}

/// The stream a change is for, and how to make it.
#[derive(Debug, Args)]
pub struct Target {
    /// A device ID (d-…) or thing name (t-…).
    pub device: String,
    /// The stream's name.
    pub stream: String,
    /// Check the input and print the request without sending it.
    #[arg(long)]
    pub dry_run: bool,
    /// Wait until the device has applied the change.
    #[arg(long)]
    pub wait: bool,
    /// How long --wait waits, in seconds.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=3600))]
    pub timeout: u64,
    /// Makes the change safe to retry (default: a new random one).
    #[arg(long)]
    pub client_token: Option<String>,
}

/// How often `--wait` reads the stream.
const POLL: Duration = Duration::from_secs(2);

/// Runs a `streams` command. `client` is `None` only for a dry run of a device ID, which needs
/// no API at all.
///
/// # Errors
///
/// The command's [`Failure`].
pub async fn run(client: Option<&Client>, command: &Streams) -> Result<Output, Failure> {
    let need = || {
        client.ok_or_else(|| Failure::Other("no client for a command that calls the API".into()))
    };
    match command {
        Streams::List { device } => {
            let client = need()?;
            let id = device_id(client, device).await?;
            let path = format!("/v1/control/devices/{}/streams", segment(&id));
            let listed = client.call(Method::Get, &path, &[], None, true).await?;
            Ok(Output::Streams(redact(listed)))
        }
        Streams::Get { device, stream } => {
            check_name("stream", stream)?;
            let client = need()?;
            let id = device_id(client, device).await?;
            Ok(Output::Stream(redact(get(client, &id, stream).await?)))
        }
        Streams::Start(target) => change(client, target, Method::Post, "/start", json!({})).await,
        Streams::Stop(target) => change(client, target, Method::Post, "/stop", json!({})).await,
        Streams::Update {
            target,
            source,
            encoding_profile,
            bitrate,
            source_ceiling_percent,
            record,
        } => {
            let mut body = serde_json::Map::new();
            if let Some(source) = source {
                body.insert("source".into(), parse_source(source)?);
            }
            if let Some(profile) = encoding_profile {
                check_name("profile", profile)?;
                body.insert("profile".into(), json!(profile));
            }
            if let Some(bitrate) = bitrate {
                body.insert("bitrate".into(), parse_bitrate(bitrate)?);
            }
            if let Some(percent) = source_ceiling_percent {
                if *percent > 400 {
                    return Err(Failure::Invalid(
                        "--source-ceiling-percent must be 0 to 400".into(),
                    ));
                }
                body.insert("sourceCeilingPercent".into(), json!(percent));
            }
            if let Some(record) = record {
                body.insert("record".into(), json!(record == "on"));
            }
            if body.is_empty() {
                return Err(Failure::Invalid(
                    "nothing to change: give --source, --encoding-profile, --bitrate, \
                     --source-ceiling-percent or --record"
                        .into(),
                ));
            }
            change(client, target, Method::Patch, "", Value::Object(body)).await
        }
        Streams::SetDestination {
            target,
            url,
            url_file,
        } => {
            let url = match (url, url_file) {
                (Some(url), _) => {
                    if url.contains("passphrase=") {
                        return Err(Failure::Invalid(
                            "a destination with a passphrase must come from --url-file (a file, \
                             or - for stdin), not the command line"
                                .into(),
                        ));
                    }
                    url.clone()
                }
                (None, Some(path)) => read_url(path)?,
                (None, None) => return Err(Failure::Invalid("give --url or --url-file".into())),
            };
            let checked = destination::parse(&url).map_err(|refused| Failure::Api {
                status: 400,
                kind: "InvalidDestination".into(),
                message: refused.message.clone(),
                members: serde_json::Map::from_iter([("part".into(), json!(refused.part))]),
            })?;
            change(
                client,
                target,
                Method::Put,
                "/destination",
                json!({"url": checked.url()}),
            )
            .await
        }
    }
}

/// Reads a URL from a file or stdin (`-`): its first line, trimmed.
fn read_url(path: &PathBuf) -> Result<String, Failure> {
    let mut text = String::new();
    let read = if path.as_os_str() == "-" {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(path).map(|found| text = found)
    };
    read.map_err(|error| Failure::Invalid(format!("{}: {error}", path.display())))?;
    let url = text.lines().next().unwrap_or_default().trim().to_owned();
    if url.is_empty() {
        return Err(Failure::Invalid(format!("{}: no URL", path.display())));
    }
    Ok(url)
}

async fn get(client: &Client, id: &str, stream: &str) -> Result<Value, Failure> {
    let path = format!(
        "/v1/control/devices/{}/streams/{}",
        segment(id),
        segment(stream)
    );
    client.call(Method::Get, &path, &[], None, true).await
}

/// Sends a change (or, with `--dry-run`, prints it), then waits if asked.
async fn change(
    client: Option<&Client>,
    target: &Target,
    method: Method,
    suffix: &str,
    mut body: Value,
) -> Result<Output, Failure> {
    check_name("stream", &target.stream)?;
    let token = match &target.client_token {
        Some(token) => {
            check_token(token)?;
            token.clone()
        }
        None => uuid::Uuid::new_v4().simple().to_string(),
    };
    body["clientToken"] = json!(token);
    let id = match client {
        Some(client) => device_id(client, &target.device).await?,
        None => target.device.clone(),
    };
    let path = format!(
        "/v1/control/devices/{}/streams/{}{suffix}",
        segment(&id),
        segment(&target.stream)
    );
    if target.dry_run {
        return Ok(Output::Json(redact(json!({
            "dryRun": true,
            "request": {"method": method.as_str(), "path": path, "body": body},
        }))));
    }
    let client = client
        .ok_or_else(|| Failure::Other("no client for a command that calls the API".into()))?;
    let changed = client.call(method, &path, &[], Some(&body), true).await?;
    if !target.wait {
        return Ok(Output::Stream(redact(changed)));
    }
    let before = changed["stream"]["rejected"].clone();
    let deadline = Instant::now() + Duration::from_secs(target.timeout);
    let mut stream = changed;
    loop {
        if let Some(refused) = newly_rejected(&before, &stream["stream"]["rejected"]) {
            return Err(Failure::Refused(format!(
                "the device refused {}: {}",
                refused["setting"].as_str().unwrap_or("the change"),
                refused["reason"].as_str().unwrap_or("no reason given"),
            )));
        }
        if stream["stream"]["pending"] == false {
            return Ok(Output::Stream(redact(stream)));
        }
        if Instant::now() >= deadline {
            return Err(Failure::TimedOut(format!(
                "the device hadn't applied the change after {} s (it may still)",
                target.timeout
            )));
        }
        tokio::time::sleep(POLL.min(deadline.saturating_duration_since(Instant::now()))).await;
        stream = get(client, &id, &target.stream).await?;
    }
}

/// A rejection in `now` that wasn't in `before`.
fn newly_rejected<'a>(before: &Value, now: &'a Value) -> Option<&'a Value> {
    let before = before.as_array().map(Vec::as_slice).unwrap_or_default();
    now.as_array()?
        .iter()
        .find(|rejected| !before.contains(rejected))
}

/// `camera:<video>[,audio=<device>]`, `rtmp:<key>`, `test[:<WxH>][@<fps>]`.
///
/// # Errors
///
/// [`Failure::Invalid`] if it isn't one of those.
pub fn parse_source(text: &str) -> Result<Value, Failure> {
    let invalid = |why: &str| Failure::Invalid(format!("--source {text:?}: {why}"));
    if let Some(rest) = text.strip_prefix("camera:") {
        let (video, audio) = match rest.split_once(",audio=") {
            Some((video, audio)) => (video, Some(audio)),
            None => (rest, None),
        };
        let usb = video.strip_prefix("usb:").is_some_and(|port| {
            let mut parts = port.split(['-', '.']);
            port.contains('-')
                && parts.all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        });
        let ids: Vec<&str> = video.splitn(3, ':').collect();
        let vid_pid = ids.len() == 3
            && ids[..2]
                .iter()
                .all(|id| id.len() == 4 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            && !ids[2].is_empty()
            && !ids[2].chars().any(char::is_whitespace);
        if !(5..=200).contains(&video.len()) || !(usb || vid_pid) {
            return Err(invalid(
                "the video is usb:<port> (usb:1-1.3) or <vid>:<pid>:<serial>",
            ));
        }
        let mut camera = json!({"video": video});
        if let Some(audio) = audio {
            if audio.is_empty() {
                return Err(invalid("audio= needs a device"));
            }
            camera["audio"] = json!(audio);
        }
        return Ok(json!({"camera": camera}));
    }
    if let Some(key) = text.strip_prefix("rtmp:") {
        let ok = key == "ingest"
            || key.strip_prefix("ingest-").is_some_and(|n| {
                (1..=3).contains(&n.len())
                    && !n.starts_with('0')
                    && n.bytes().all(|b| b.is_ascii_digit())
            });
        if !ok {
            return Err(invalid("the RTMP key is ingest or ingest-<n>"));
        }
        return Ok(json!({"rtmp": {"key": key}}));
    }
    if let Some(rest) = text.strip_prefix("test") {
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        let (size, fps) = match rest.split_once('@') {
            Some((size, fps)) => (size, Some(fps)),
            None => (rest, None),
        };
        let mut test = serde_json::Map::new();
        if !size.is_empty() {
            let ok = size.split_once('x').is_some_and(|(w, h)| {
                [w, h]
                    .iter()
                    .all(|n| (2..=4).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
            });
            if !ok {
                return Err(invalid("the size is WIDTHxHEIGHT, such as 1280x720"));
            }
            test.insert("size".into(), json!(size));
        }
        if let Some(fps) = fps {
            let fps: u8 = fps
                .parse()
                .ok()
                .filter(|fps| (1..=120).contains(fps))
                .ok_or_else(|| invalid("fps is 1 to 120"))?;
            test.insert("fps".into(), json!(fps));
        }
        return Ok(json!({"test": test}));
    }
    Err(invalid(
        "it's camera:<video>[,audio=<device>], rtmp:<key> or test[:<WxH>][@<fps>]",
    ))
}

/// A kbit/s figure (100–100000), or `default`.
///
/// # Errors
///
/// [`Failure::Invalid`] if it's neither.
pub fn parse_bitrate(text: &str) -> Result<Value, Failure> {
    if text == "default" {
        return Ok(json!({"profileDefault": {}}));
    }
    text.parse::<u32>()
        .ok()
        .filter(|kbps| (100..=100_000).contains(kbps))
        .map(|kbps| json!({"overrideKbps": kbps}))
        .ok_or_else(|| {
            Failure::Invalid(format!(
                "--bitrate {text:?}: kbit/s from 100 to 100000, or default"
            ))
        })
}

/// A stream or profile name: 1–64 letters, digits, `-` and `_`.
fn check_name(what: &str, name: &str) -> Result<(), Failure> {
    let ok = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(Failure::Invalid(format!(
            "{what} {name:?}: 1 to 64 letters, digits, - and _"
        )))
    }
}

fn check_token(token: &str) -> Result<(), Failure> {
    check_name("client token", token)
}

/// `value` with every SRT passphrase in a destination URL replaced by `***`: the API returns
/// destinations as set, and heliosctl never prints a secret.
#[must_use]
pub fn redact(value: Value) -> Value {
    match value {
        Value::String(text) if text.starts_with("srt://") || text.starts_with("srtla://") => {
            Value::String(redact_url(&text))
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redact).collect()),
        Value::Object(map) => Value::Object(map.into_iter().map(|(k, v)| (k, redact(v))).collect()),
        other => other,
    }
}

fn redact_url(url: &str) -> String {
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
mod tests;
