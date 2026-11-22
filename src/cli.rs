//! The command line (HLD 7.3.4.1): parsing, running a command, and printing its result or
//! failure. Never interactive: everything is a flag or stdin, so a run completes or fails.

use std::ffi::OsString;
use std::io::Write;

use clap::{Parser, Subcommand};
use serde_json::{Value, json};

use crate::client::{Client, Method};
use crate::error::{Failure, exit};
use crate::{config, output};

/// The Control API's command-line client, for operators and AI agents.
#[derive(Debug, Parser)]
#[command(
    name = "heliosctl",
    version,
    about = "The Helios Control API's command-line client, for operators and AI agents",
    long_about = "The Helios Control API's command-line client, for operators and AI agents \
                  (HLD 7.3.4.1). Requests are signed with your AWS credentials (an AWS profile \
                  that assumes helios-operator). Every command takes --json for one JSON \
                  document on stdout; errors then go to stderr as JSON, and the exit code says \
                  what happened: 0 done, 1 other, 2 invalid input, 3 not found, 4 refused, \
                  5 timed out, 6 not authorized, 7 not reported yet, 8 throttled."
)]
pub struct Cli {
    /// Print one JSON document on stdout, and errors as JSON on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    /// The Control API's base URL (…/dev).
    #[arg(long, global = true, env = "HELIOS_CONTROL_URL")]
    pub endpoint: Option<String>,
    /// The AWS profile: one that assumes helios-operator [default: helios-operator].
    #[arg(long, global = true, env = "AWS_PROFILE")]
    pub profile: Option<String>,
    /// The AWS region [default: the profile's].
    #[arg(long, global = true, env = "AWS_REGION")]
    pub region: Option<String>,
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands, one per Control API operation.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Devices in the bootstrap service.
    #[command(subcommand)]
    Devices(Devices),
    /// The Control API's operations, their inputs and outputs (OpenAPI), as JSON.
    Schema {
        /// One operation, such as SetWifiNetwork.
        operation: Option<String>,
    },
}

/// `heliosctl devices`.
#[derive(Debug, Subcommand)]
pub enum Devices {
    /// List every device.
    List,
    /// One device: its record, connection, Helios version and modules.
    ///
    /// The device is its device ID (d-…) or its thing name (t-…).
    Get {
        /// A device ID or thing name.
        device: String,
    },
}

/// The OpenAPI description of the Control API, from helios-contract's release.
const OPENAPI: &str = include_str!("../schema/HeliosControl.openapi.json");

/// Runs `heliosctl` with `args` and returns the exit code; output goes to `out`, errors to `err`.
pub async fn run(
    args: impl IntoIterator<Item = OsString>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            let code = if error.use_stderr() {
                exit::INVALID
            } else {
                exit::OK
            };
            let target: &mut dyn Write = if error.use_stderr() { err } else { out };
            let _ = write!(target, "{}", error.render());
            return code;
        }
    };
    let json = cli.json;
    match execute(cli).await {
        Ok(result) => {
            let _ = writeln!(out, "{}", result.render(json));
            exit::OK
        }
        Err(failure) => {
            if json {
                let _ = writeln!(err, "{}", failure.to_json());
            } else {
                let _ = writeln!(err, "heliosctl: {failure}");
            }
            failure.exit_code()
        }
    }
}

async fn execute(cli: Cli) -> Result<output::Output, Failure> {
    match &cli.command {
        Command::Schema { operation } => schema(operation.as_deref()),
        Command::Devices(command) => {
            let client = connect(&cli).await?;
            devices(&client, command).await
        }
    }
}

/// A client from the settings and the AWS credential chain.
async fn connect(cli: &Cli) -> Result<Client, Failure> {
    let file = match config::default_path() {
        Some(path) => config::read_file(&path)?,
        None => config::File::default(),
    };
    let settings = config::resolve(
        cli.endpoint.clone(),
        cli.profile.clone(),
        cli.region.clone(),
        file,
    )?;
    let mut loader =
        aws_config::defaults(aws_config::BehaviorVersion::latest()).profile_name(&settings.profile);
    if let Some(region) = &settings.region {
        loader = loader.region(aws_config::Region::new(region.clone()));
    }
    let aws = loader.load().await;
    let region = aws.region().map(ToString::to_string).ok_or_else(|| {
        Failure::Invalid("no AWS region: give --region or set it in the profile".into())
    })?;
    let credentials = aws
        .credentials_provider()
        .ok_or_else(|| Failure::Credentials(format!("profile {:?} has none", settings.profile)))?;
    Client::new(&settings.endpoint, &region, credentials)
}

async fn devices(client: &Client, command: &Devices) -> Result<output::Output, Failure> {
    match command {
        Devices::List => {
            let devices = list_devices(client).await?;
            Ok(output::Output::Devices(json!({"devices": devices})))
        }
        Devices::Get { device } => {
            let id = device_id(client, device).await?;
            let found = client
                .call(
                    Method::Get,
                    &format!("/v1/control/devices/{}", segment(&id)),
                    &[],
                    None,
                    true,
                )
                .await?;
            Ok(output::Output::Device(found))
        }
    }
}

/// Every device, following `nextToken`.
async fn list_devices(client: &Client) -> Result<Vec<Value>, Failure> {
    let mut devices = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut query = vec![("maxResults", "100")];
        if let Some(token) = &token {
            query.push(("nextToken", token));
        }
        let page = client
            .call(Method::Get, "/v1/control/devices", &query, None, true)
            .await?;
        devices.extend(page["devices"].as_array().cloned().unwrap_or_default());
        match page["nextToken"].as_str() {
            Some(next) if !next.is_empty() => token = Some(next.to_owned()),
            _ => return Ok(devices),
        }
    }
}

/// A device's ID: `name` itself if it's one (`d-…`), else the device whose thing name it is.
///
/// # Errors
///
/// [`Failure::Api`] `DeviceNotFound` if no device has that thing name.
pub async fn device_id(client: &Client, name: &str) -> Result<String, Failure> {
    if name.starts_with("d-") {
        return Ok(name.to_owned());
    }
    list_devices(client)
        .await?
        .iter()
        .find(|device| device["thingName"] == name)
        .and_then(|device| device["deviceId"].as_str().map(str::to_owned))
        .ok_or_else(|| Failure::Api {
            status: 404,
            kind: "DeviceNotFound".into(),
            message: format!("No device with ID or thing name {name}."),
            members: serde_json::Map::new(),
        })
}

/// A path segment, percent-encoded.
#[must_use]
pub fn segment(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// `heliosctl schema [operation]`.
fn schema(operation: Option<&str>) -> Result<output::Output, Failure> {
    let document: Value = serde_json::from_str(OPENAPI)
        .map_err(|error| Failure::Other(format!("the bundled OpenAPI: {error}")))?;
    let Some(operation) = operation else {
        return Ok(output::Output::Json(document));
    };
    for (path, methods) in document["paths"].as_object().into_iter().flatten() {
        for (method, details) in methods.as_object().into_iter().flatten() {
            if details["operationId"] == operation {
                return Ok(output::Output::Json(json!({
                    "operation": operation,
                    "method": method.to_uppercase(),
                    "path": path,
                    "details": details,
                    "components": document["components"],
                })));
            }
        }
    }
    Err(Failure::Invalid(format!(
        "no operation {operation}: `heliosctl schema` lists them all"
    )))
}

#[cfg(test)]
mod tests;
