//! Failures and their exit codes (HLD 7.3.4.1): documented, so an agent can act on them without
//! reading the message.

use serde_json::{Map, Value, json};

/// Exit codes.
pub mod exit {
    /// Done (and, with `--wait`, applied).
    pub const OK: u8 = 0;
    /// Anything else: a network error, an internal error, a response that doesn't parse.
    pub const OTHER: u8 = 1;
    /// Invalid input: usage errors, `ValidationException`, `InvalidDestination`, `--dry-run`.
    pub const INVALID: u8 = 2;
    /// Not found: `DeviceNotFound`, `StreamNotFound`.
    pub const NOT_FOUND: u8 = 3;
    /// Refused: `SettingUnavailable`, `WifiNetworkRefused`, or the device's `rejected`.
    pub const REFUSED: u8 = 4;
    /// The device didn't answer or apply in time: `DeviceUnreachable`, `--wait` timing out.
    pub const TIMED_OUT: u8 = 5;
    /// Not authorized: no credentials, or HTTP 403.
    pub const NOT_AUTHORIZED: u8 = 6;
    /// The device hasn't reported that: `DeviceStateUnavailable`.
    pub const UNAVAILABLE: u8 = 7;
    /// Throttled: `RateLimited`, after retries.
    pub const THROTTLED: u8 = 8;
}

/// Why a command failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Failure {
    /// The command line or its values are wrong.
    #[error("{0}")]
    Invalid(String),
    /// The API answered with an error.
    #[error("{kind}: {message}")]
    Api {
        /// HTTP status.
        status: u16,
        /// The error's type, from `X-Amzn-Errortype` (`WifiNetworkRefused`).
        kind: String,
        /// The error's message.
        message: String,
        /// The error's other members (`reason`, `setting`, `part`).
        members: Map<String, Value>,
    },
    /// No usable AWS credentials.
    #[error("no AWS credentials: {0}")]
    Credentials(String),
    /// The device didn't apply a change in time (`--wait`).
    #[error("{0}")]
    TimedOut(String),
    /// The device refused a change it was waiting for (`--wait`).
    #[error("{0}")]
    Refused(String),
    /// The API couldn't be reached, or answered with something that doesn't parse.
    #[error("{0}")]
    Other(String),
}

impl Failure {
    /// The exit code for it.
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Invalid(_) => exit::INVALID,
            Self::Credentials(_) => exit::NOT_AUTHORIZED,
            Self::TimedOut(_) => exit::TIMED_OUT,
            Self::Refused(_) => exit::REFUSED,
            Self::Other(_) => exit::OTHER,
            Self::Api { status, kind, .. } => match kind.as_str() {
                "ValidationException" | "InvalidDestination" => exit::INVALID,
                "DeviceNotFound" | "StreamNotFound" => exit::NOT_FOUND,
                "SettingUnavailable" | "WifiNetworkRefused" => exit::REFUSED,
                "DeviceUnreachable" => exit::TIMED_OUT,
                "DeviceStateUnavailable" => exit::UNAVAILABLE,
                "RateLimited" => exit::THROTTLED,
                _ if *status == 403 || *status == 401 => exit::NOT_AUTHORIZED,
                _ => exit::OTHER,
            },
        }
    }

    /// Its type, as `--json` errors name it.
    #[must_use]
    pub fn kind(&self) -> &str {
        match self {
            Self::Invalid(_) => "InvalidInput",
            Self::Credentials(_) => "NoCredentials",
            Self::TimedOut(_) => "TimedOut",
            Self::Refused(_) => "Refused",
            Self::Other(_) => "Error",
            Self::Api { kind, .. } => kind,
        }
    }

    /// `{"error": {"type", "message", ...the error's members}}`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut error = Map::new();
        error.insert("type".into(), json!(self.kind()));
        let message = match self {
            Self::Api { message, .. } => message.clone(),
            other => other.to_string(),
        };
        error.insert("message".into(), json!(message));
        if let Self::Api {
            members, status, ..
        } = self
        {
            for (name, value) in members {
                if name != "message" {
                    error.insert(name.clone(), value.clone());
                }
            }
            error.insert("status".into(), json!(status));
        }
        json!({"error": error})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, kind: &str) -> Failure {
        Failure::Api {
            status,
            kind: kind.into(),
            message: "why".into(),
            members: Map::new(),
        }
    }

    #[test]
    fn maps_each_failure_to_its_documented_code() {
        for (failure, code) in [
            (Failure::Invalid("x".into()), 2),
            (api(400, "ValidationException"), 2),
            (api(400, "InvalidDestination"), 2),
            (api(404, "DeviceNotFound"), 3),
            (api(404, "StreamNotFound"), 3),
            (api(409, "SettingUnavailable"), 4),
            (api(409, "WifiNetworkRefused"), 4),
            (Failure::Refused("x".into()), 4),
            (api(504, "DeviceUnreachable"), 5),
            (Failure::TimedOut("x".into()), 5),
            (api(403, "AccessDeniedException"), 6),
            (api(401, "UnrecognizedClientException"), 6),
            (Failure::Credentials("x".into()), 6),
            (api(409, "DeviceStateUnavailable"), 7),
            (api(429, "RateLimited"), 8),
            (api(500, "InternalError"), 1),
            (Failure::Other("x".into()), 1),
        ] {
            assert_eq!(failure.exit_code(), code, "{failure:?}");
        }
    }

    #[test]
    fn json_errors_carry_the_type_message_and_members() {
        let mut members = Map::new();
        members.insert("reason".into(), json!("no_adapter"));
        members.insert(
            "message".into(),
            json!("ignored: the message is its own field"),
        );
        let failure = Failure::Api {
            status: 409,
            kind: "WifiNetworkRefused".into(),
            message: "The device has no Wi-Fi adapter.".into(),
            members,
        };
        assert_eq!(
            failure.to_json(),
            json!({"error": {
                "type": "WifiNetworkRefused",
                "message": "The device has no Wi-Fi adapter.",
                "reason": "no_adapter",
                "status": 409,
            }})
        );
        assert_eq!(
            Failure::TimedOut("not applied in 60 s".into()).to_json(),
            json!({"error": {"type": "TimedOut", "message": "not applied in 60 s"}})
        );
        for (failure, kind) in [
            (Failure::Invalid("x".into()), "InvalidInput"),
            (Failure::Credentials("x".into()), "NoCredentials"),
            (Failure::Refused("x".into()), "Refused"),
            (Failure::Other("x".into()), "Error"),
        ] {
            assert_eq!(failure.kind(), kind);
        }
        assert_eq!(
            api(404, "DeviceNotFound").to_string(),
            "DeviceNotFound: why"
        );
    }
}
