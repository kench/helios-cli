//! `heliosctl inputs`, `profiles` and `wifi` (HLD 7.3.4.1): what a device can stream from, its
//! encoding profiles, and the Wi-Fi networks it joins.
//!
//! A Wi-Fi passphrase is read from a file or stdin, never the command line, and never printed:
//! the API takes it over TLS and seals it for the device.

use std::io::Read;
use std::path::PathBuf;

use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::{device_id, segment};
use crate::client::{Client, Method};
use crate::error::Failure;
use crate::output::Output;

/// `heliosctl inputs`.
#[derive(Debug, Subcommand)]
pub enum Inputs {
    /// What the device can stream from: cameras, audio devices, RTMP ingest keys, test patterns.
    List {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
    },
}

/// `heliosctl profiles`.
#[derive(Debug, Subcommand)]
pub enum Profiles {
    /// The device's encoding profiles.
    List {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
    },
}

/// `heliosctl wifi`.
#[derive(Debug, Subcommand)]
pub enum Wifi {
    /// The Wi-Fi networks the device is set to join (never their passphrases).
    List {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
    },
    /// Add a network, or change one: the device applies it before this returns.
    Set {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
        /// The network's name (SSID), at most 32 bytes.
        #[arg(long)]
        ssid: String,
        /// A file holding the WPA2/WPA3-Personal passphrase, or - for stdin.
        #[arg(long, conflicts_with = "open", required_unless_present = "open")]
        passphrase_file: Option<PathBuf>,
        /// An open network, with no passphrase.
        #[arg(long)]
        open: bool,
        /// The network doesn't broadcast its SSID.
        #[arg(long)]
        hidden: bool,
        /// Check the input and print the request (passphrase hidden) without sending it.
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a network.
    Remove {
        /// A device ID (d-…) or thing name (t-…).
        device: String,
        /// The network's name (SSID).
        #[arg(long)]
        ssid: String,
        /// Print the request without sending it.
        #[arg(long)]
        dry_run: bool,
    },
}

fn path(id: &str, suffix: &str) -> String {
    format!("/v1/control/devices/{}{suffix}", segment(id))
}

/// Runs `inputs list`.
///
/// # Errors
///
/// The API's [`Failure`].
pub async fn inputs(client: &Client, command: &Inputs) -> Result<Output, Failure> {
    let Inputs::List { device } = command;
    let id = device_id(client, device).await?;
    let found = client
        .call(Method::Get, &path(&id, "/inputs"), &[], None, true)
        .await?;
    Ok(Output::Inputs(found))
}

/// Runs `profiles list`.
///
/// # Errors
///
/// The API's [`Failure`].
pub async fn profiles(client: &Client, command: &Profiles) -> Result<Output, Failure> {
    let Profiles::List { device } = command;
    let id = device_id(client, device).await?;
    let found = client
        .call(Method::Get, &path(&id, "/profiles"), &[], None, true)
        .await?;
    Ok(Output::Profiles(found))
}

/// Whether a `wifi` command is a dry run for a device ID, which needs no API.
#[must_use]
pub fn offline(command: &Wifi) -> bool {
    match command {
        Wifi::Set {
            device, dry_run, ..
        }
        | Wifi::Remove {
            device, dry_run, ..
        } => *dry_run && device.starts_with("d-"),
        Wifi::List { .. } => false,
    }
}

/// Runs a `wifi` command. `client` is `None` only for an [`offline`] dry run.
///
/// # Errors
///
/// The command's [`Failure`].
pub async fn wifi(client: Option<&Client>, command: &Wifi) -> Result<Output, Failure> {
    let need = || {
        client.ok_or_else(|| Failure::Other("no client for a command that calls the API".into()))
    };
    let resolve = |device: &str| {
        let device = device.to_owned();
        async move {
            match client {
                Some(client) => device_id(client, &device).await,
                None => Ok(device),
            }
        }
    };
    match command {
        Wifi::List { device } => {
            let client = need()?;
            let id = device_id(client, device).await?;
            let found = client
                .call(Method::Get, &path(&id, "/wifi/networks"), &[], None, true)
                .await?;
            Ok(Output::Wifi(found))
        }
        Wifi::Set {
            device,
            ssid,
            passphrase_file,
            open: _,
            hidden,
            dry_run,
        } => {
            check_ssid(ssid)?;
            let mut body = json!({"ssid": ssid, "hidden": hidden});
            if let Some(file) = passphrase_file {
                let passphrase = read_passphrase(file)?;
                check_passphrase(&passphrase)?;
                body["passphrase"] = json!(passphrase);
            }
            let id = resolve(device).await?;
            let path = path(&id, "/wifi/networks");
            if *dry_run {
                let mut shown = body.clone();
                if shown.get("passphrase").is_some() {
                    shown["passphrase"] = json!("***");
                }
                return Ok(Output::Json(json!({
                    "dryRun": true,
                    "request": {"method": "PUT", "path": path, "body": shown},
                })));
            }
            let set = need()?
                .call(Method::Put, &path, &[], Some(&body), true)
                .await?;
            Ok(Output::Json(set))
        }
        Wifi::Remove {
            device,
            ssid,
            dry_run,
        } => {
            check_ssid(ssid)?;
            let id = resolve(device).await?;
            let path = path(&id, "/wifi/networks");
            if *dry_run {
                return Ok(Output::Json(json!({
                    "dryRun": true,
                    "request": {"method": "DELETE", "path": path, "query": {"ssid": ssid}},
                })));
            }
            let removed = need()?
                .call(Method::Delete, &path, &[("ssid", ssid)], None, true)
                .await?;
            Ok(Output::Json(removed))
        }
    }
}

/// The passphrase: a file's (or stdin's) first line, without its line ending. Spaces are kept:
/// they can be part of a passphrase.
fn read_passphrase(file: &PathBuf) -> Result<String, Failure> {
    let mut text = String::new();
    let read = if file.as_os_str() == "-" {
        std::io::stdin().read_to_string(&mut text).map(|_| ())
    } else {
        std::fs::read_to_string(file).map(|found| text = found)
    };
    read.map_err(|error| Failure::Invalid(format!("{}: {error}", file.display())))?;
    Ok(text
        .split('\n')
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r')
        .to_owned())
}

/// 1–32 bytes.
fn check_ssid(ssid: &str) -> Result<(), Failure> {
    if (1..=32).contains(&ssid.len()) {
        Ok(())
    } else {
        Err(Failure::Invalid(format!(
            "--ssid: 1 to 32 bytes, not {}",
            ssid.len()
        )))
    }
}

/// WPA2/WPA3-Personal: 8–63 printable ASCII characters, or 64 hex digits. The message never
/// includes the passphrase.
fn check_passphrase(passphrase: &str) -> Result<(), Failure> {
    let printable = (8..=63).contains(&passphrase.len())
        && passphrase.bytes().all(|b| (b' '..=b'~').contains(&b));
    let hex = passphrase.len() == 64 && passphrase.bytes().all(|b| b.is_ascii_hexdigit());
    if printable || hex {
        Ok(())
    } else {
        Err(Failure::Invalid(
            "the passphrase must be 8 to 63 printable ASCII characters, or 64 hex digits".into(),
        ))
    }
}

/// `{"camera": {...}}` as (`camera`, the details).
#[must_use]
pub fn kind(input: &Value) -> (String, &Value) {
    match input.as_object().and_then(|map| map.iter().next()) {
        Some((kind, details)) => (kind.clone(), details),
        None => ("-".into(), &Value::Null),
    }
}

#[cfg(test)]
mod tests;
