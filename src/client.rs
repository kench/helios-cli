//! The Control API over HTTPS, each request SigV4-signed for `execute-api` with credentials from
//! the AWS credential chain (HLD 7.3.4.1).

use std::time::{Duration, SystemTime};

use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use aws_sigv4::sign::v4;
use serde_json::{Map, Value};

use crate::error::Failure;

/// How long one request may take.
const TIMEOUT: Duration = Duration::from_secs(30);
/// Reads and token-carrying writes are retried this many times on throttling or network errors.
const RETRIES: u32 = 3;

/// A Control API client.
#[derive(Debug, Clone)]
pub struct Client {
    endpoint: String,
    region: String,
    credentials: SharedCredentialsProvider,
    http: reqwest::Client,
}

/// An HTTP method the API uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Reads.
    Get,
    /// Starts and stops.
    Post,
    /// Replacements: destinations, Wi-Fi networks.
    Put,
    /// Updates.
    Patch,
    /// Removals.
    Delete,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    fn reqwest(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Patch => reqwest::Method::PATCH,
            Self::Delete => reqwest::Method::DELETE,
        }
    }
}

impl Client {
    /// A client for the API at `endpoint` in `region`, signing with `credentials`.
    ///
    /// # Errors
    ///
    /// [`Failure::Other`] if the HTTP client can't be built.
    pub fn new(
        endpoint: &str,
        region: &str,
        credentials: SharedCredentialsProvider,
    ) -> Result<Self, Failure> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("heliosctl/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| Failure::Other(format!("HTTP client: {error}")))?;
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            region: region.to_owned(),
            credentials,
            http,
        })
    }

    /// Calls `method path` with `query` and a JSON `body`, and returns the JSON response (or
    /// `{}` for an empty one). `retry` allows retries: for reads, and writes that carry a client
    /// token, so a retry is the same change.
    ///
    /// # Errors
    ///
    /// [`Failure::Api`] for the API's errors, [`Failure::Credentials`] without credentials, and
    /// [`Failure::Other`] for network errors or a response that isn't JSON.
    pub async fn call(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
        retry: bool,
    ) -> Result<Value, Failure> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.once(method, path, query, body).await {
                Err(failure) if retry && attempt <= RETRIES && retryable(&failure) => {
                    tokio::time::sleep(Duration::from_millis(200 * 2u64.pow(attempt))).await;
                }
                result => return result,
            }
        }
    }

    async fn once(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<Value, Failure> {
        let mut url = reqwest::Url::parse(&format!("{}{path}", self.endpoint))
            .map_err(|error| Failure::Invalid(format!("{path}: {error}")))?;
        if !query.is_empty() {
            url.query_pairs_mut().extend_pairs(query);
        }
        let payload = match body {
            Some(body) => serde_json::to_vec(body)
                .map_err(|error| Failure::Other(format!("request body: {error}")))?,
            None => Vec::new(),
        };
        let mut headers = vec![("host".to_owned(), host(&url))];
        if body.is_some() {
            headers.push(("content-type".to_owned(), "application/json".to_owned()));
        }
        let signed = self.sign(method, url.as_str(), &headers, &payload).await?;
        let mut request = self.http.request(method.reqwest(), url);
        for (name, value) in headers.iter().chain(&signed) {
            if name != "host" {
                request = request.header(name, value);
            }
        }
        let response =
            request.body(payload).send().await.map_err(|error| {
                Failure::Other(format!("the Control API didn't answer: {error}"))
            })?;
        let status = response.status().as_u16();
        let kind = response
            .headers()
            .get("x-amzn-errortype")
            .and_then(|value| value.to_str().ok())
            .map(|value| value.split(':').next().unwrap_or(value).to_owned());
        let text = response
            .text()
            .await
            .map_err(|error| Failure::Other(format!("reading the response: {error}")))?;
        parse(status, kind, &text)
    }

    /// The SigV4 headers for a request.
    async fn sign(
        &self,
        method: Method,
        url: &str,
        headers: &[(String, String)],
        payload: &[u8],
    ) -> Result<Vec<(String, String)>, Failure> {
        let credentials = self
            .credentials
            .provide_credentials()
            .await
            .map_err(|error| Failure::Credentials(error.to_string()))?;
        let identity = credentials.into();
        let params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name("execute-api")
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|error| Failure::Other(format!("signing: {error}")))?
            .into();
        let signable = SignableRequest::new(
            method.as_str(),
            url,
            headers
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
            SignableBody::Bytes(payload),
        )
        .map_err(|error| Failure::Other(format!("signing: {error}")))?;
        let (instructions, _) = sign(signable, &params)
            .map_err(|error| Failure::Other(format!("signing: {error}")))?
            .into_parts();
        Ok(instructions
            .headers()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect())
    }
}

fn host(url: &reqwest::Url) -> String {
    match (url.host_str(), url.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_owned(),
        (None, _) => String::new(),
    }
}

/// A response as JSON, or the API's error.
fn parse(status: u16, kind: Option<String>, text: &str) -> Result<Value, Failure> {
    let body: Value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|_| {
            Failure::Other(format!(
                "the Control API answered {status} with something that isn't JSON"
            ))
        })?
    };
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let members = body.as_object().cloned().unwrap_or_default();
    let message = members
        .get("message")
        .or_else(|| members.get("Message"))
        .and_then(Value::as_str)
        .unwrap_or("no message")
        .to_owned();
    let kind = kind.unwrap_or_else(|| match status {
        403 => "AccessDeniedException".into(),
        404 => "NotFound".into(),
        429 => "RateLimited".into(),
        _ => format!("HTTP{status}"),
    });
    Err(Failure::Api {
        status,
        kind,
        message,
        members,
    })
}

fn retryable(failure: &Failure) -> bool {
    match failure {
        Failure::Other(_) => true,
        Failure::Api { status, kind, .. } => {
            kind == "RateLimited" || *status == 429 || *status >= 502
        }
        _ => false,
    }
}

#[cfg(test)]
pub(crate) mod tests;
