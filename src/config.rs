//! Where the Control API is, and as whom to call it (HLD 7.3.4.1).
//!
//! Each setting comes from its flag, then its environment variable, then
//! `~/.config/heliosctl/config.toml`:
//!
//! ```toml
//! endpoint = "https://abc123.execute-api.us-west-2.amazonaws.com/dev"
//! profile = "helios-operator"   # an AWS profile that assumes helios-operator
//! region = "us-west-2"
//! ```
//!
//! None of these is a secret: credentials come from the AWS credential chain.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::Failure;

/// The AWS profile used when none is given: one that assumes the `helios-operator` role.
pub const DEFAULT_PROFILE: &str = "helios-operator";

/// The settings file.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    endpoint: Option<String>,
    profile: Option<String>,
    region: Option<String>,
}

/// The settings to call the API with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The API's base URL, without a trailing `/`: `…/dev`; routes start `/v1/control`.
    pub endpoint: String,
    /// The AWS profile.
    pub profile: String,
    /// The AWS region, if set; otherwise the profile's.
    pub region: Option<String>,
}

/// The default settings file, `$XDG_CONFIG_HOME/heliosctl/config.toml`.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("heliosctl").join("config.toml"))
}

/// Reads the settings file; a missing one is empty.
///
/// # Errors
///
/// [`Failure::Invalid`] if it can't be read or doesn't parse.
pub fn read_file(path: &Path) -> Result<File, Failure> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text)
            .map_err(|error| Failure::Invalid(format!("{}: {error}", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
        Err(error) => Err(Failure::Invalid(format!("{}: {error}", path.display()))),
    }
}

/// The settings from the flags or environment (`given`, as clap read them) and the file.
///
/// # Errors
///
/// [`Failure::Invalid`] if there's no endpoint, or it isn't an `https://` URL (or `http://` to
/// the local machine, for tests).
pub fn resolve(
    endpoint: Option<String>,
    profile: Option<String>,
    region: Option<String>,
    file: File,
) -> Result<Settings, Failure> {
    let endpoint = endpoint.or(file.endpoint).ok_or_else(|| {
        Failure::Invalid(
            "no Control API endpoint: give --endpoint, set HELIOS_CONTROL_URL, or put endpoint in \
             ~/.config/heliosctl/config.toml"
                .into(),
        )
    })?;
    let endpoint = endpoint.trim_end_matches('/').to_owned();
    check_endpoint(&endpoint)?;
    Ok(Settings {
        endpoint,
        profile: profile
            .or(file.profile)
            .unwrap_or_else(|| DEFAULT_PROFILE.to_owned()),
        region: region.or(file.region),
    })
}

fn check_endpoint(endpoint: &str) -> Result<(), Failure> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|error| Failure::Invalid(format!("endpoint {endpoint:?}: {error}")))?;
    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    let secure = url.scheme() == "https" || (url.scheme() == "http" && local);
    if !secure || url.query().is_some() || url.fragment().is_some() {
        return Err(Failure::Invalid(format!(
            "endpoint {endpoint:?} must be an https:// URL without a query"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://abc123.execute-api.us-west-2.amazonaws.com/dev";

    #[test]
    fn flags_win_over_the_file_and_defaults_fill_in() {
        let file = File {
            endpoint: Some("https://file.example.com/dev".into()),
            profile: Some("ops".into()),
            region: Some("eu-west-1".into()),
        };
        let given = resolve(
            Some(format!("{URL}/")),
            Some("mine".into()),
            Some("us-west-2".into()),
            file.clone(),
        )
        .unwrap();
        assert_eq!(given.endpoint, URL, "the trailing slash goes");
        assert_eq!(given.profile, "mine");
        assert_eq!(given.region.as_deref(), Some("us-west-2"));
        let from_file = resolve(None, None, None, file).unwrap();
        assert_eq!(from_file.endpoint, "https://file.example.com/dev");
        assert_eq!(from_file.profile, "ops");
        let defaults = resolve(Some(URL.into()), None, None, File::default()).unwrap();
        assert_eq!(defaults.profile, DEFAULT_PROFILE);
        assert_eq!(defaults.region, None);
    }

    #[test]
    fn needs_a_secure_endpoint() {
        assert!(matches!(
            resolve(None, None, None, File::default()),
            Err(Failure::Invalid(message)) if message.contains("HELIOS_CONTROL_URL")
        ));
        for bad in [
            "http://abc.execute-api.us-west-2.amazonaws.com/dev",
            "https://x.example.com/dev?a=1",
            "https://x.example.com/dev#here",
            "not a url",
            "ftp://x.example.com",
        ] {
            assert!(
                resolve(Some(bad.into()), None, None, File::default()).is_err(),
                "{bad}"
            );
        }
        for local in [
            "http://127.0.0.1:8080",
            "http://localhost:1/x",
            "http://[::1]:9",
        ] {
            assert!(resolve(Some(local.into()), None, None, File::default()).is_ok());
        }
    }

    #[test]
    fn reads_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(
            read_file(&path).unwrap(),
            File::default(),
            "missing is empty"
        );
        std::fs::write(&path, format!("endpoint = \"{URL}\"\nprofile = \"ops\"\n")).unwrap();
        let file = read_file(&path).unwrap();
        assert_eq!(file.endpoint.as_deref(), Some(URL));
        std::fs::write(&path, "endpont = \"typo\"\n").unwrap();
        assert!(matches!(read_file(&path), Err(Failure::Invalid(_))));
        assert!(
            matches!(read_file(dir.path()), Err(Failure::Invalid(_))),
            "a directory"
        );
        assert!(default_path().is_none_or(|path| path.ends_with("heliosctl/config.toml")));
    }
}
