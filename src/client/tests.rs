use std::sync::{Arc, Mutex};

use aws_credential_types::Credentials;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

/// A request the mock server received.
#[derive(Debug, Clone)]
pub(crate) struct Received {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: String,
}

impl Received {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// A canned response: status, extra headers, body.
pub(crate) type Canned = (u16, Vec<(&'static str, &'static str)>, String);

/// A one-connection-at-a-time HTTP/1.1 server answering from `responses` in order (the last
/// one repeats), recording what it receives.
pub(crate) async fn server(responses: Vec<Canned>) -> (String, Arc<Mutex<Vec<Received>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&received);
    tokio::spawn(async move {
        let mut index = 0;
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buffer = Vec::new();
            let mut chunk = [0u8; 4096];
            let (head_end, length) = loop {
                let n = stream.read(&mut chunk).await.unwrap_or(0);
                if n == 0 {
                    break (None, 0);
                }
                buffer.extend_from_slice(&chunk[..n]);
                if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buffer[..end]).to_string();
                    let length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while buffer.len() < end + 4 + length {
                        let n = stream.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        buffer.extend_from_slice(&chunk[..n]);
                    }
                    break (Some(end), length);
                }
            };
            let Some(end) = head_end else {
                continue;
            };
            let head = String::from_utf8_lossy(&buffer[..end]).to_string();
            let mut lines = head.lines();
            let request_line: Vec<&str> = lines.next().unwrap_or_default().split(' ').collect();
            let headers = lines
                .filter_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    Some((name.trim().to_owned(), value.trim().to_owned()))
                })
                .collect();
            let body =
                String::from_utf8_lossy(&buffer[end + 4..(end + 4 + length).min(buffer.len())])
                    .to_string();
            log.lock().unwrap().push(Received {
                method: request_line.first().copied().unwrap_or_default().to_owned(),
                target: request_line.get(1).copied().unwrap_or_default().to_owned(),
                headers,
                body,
            });
            let (status, extra, body) = responses[index.min(responses.len() - 1)].clone();
            index += 1;
            let mut response = format!(
                "HTTP/1.1 {status} X\r\ncontent-length: {}\r\ncontent-type: application/json\r\nconnection: close\r\n",
                body.len()
            );
            for (name, value) in extra {
                response.push_str(&format!("{name}: {value}\r\n"));
            }
            response.push_str("\r\n");
            response.push_str(&body);
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    (format!("http://{address}/dev"), received)
}

pub(crate) fn credentials() -> SharedCredentialsProvider {
    SharedCredentialsProvider::new(Credentials::new(
        "AKIDEXAMPLE",
        "secret",
        Some("session-token".into()),
        None,
        "test",
    ))
}

pub(crate) fn client(endpoint: &str) -> Client {
    Client::new(endpoint, "us-west-2", credentials()).unwrap()
}

#[tokio::test]
async fn signs_each_request_for_execute_api() {
    let (endpoint, received) = server(vec![(200, vec![], r#"{"devices": []}"#.into())]).await;
    let body = json!({"ssid": "Venue / Media"});
    let answer = client(&endpoint)
        .call(
            Method::Put,
            "/v1/control/devices/d-1/wifi/networks",
            &[("ssid", "Venue / Media")],
            Some(&body),
            false,
        )
        .await
        .unwrap();
    assert_eq!(answer, json!({"devices": []}));
    let request = received.lock().unwrap()[0].clone();
    assert_eq!(request.method, "PUT");
    assert_eq!(
        request.target,
        "/dev/v1/control/devices/d-1/wifi/networks?ssid=Venue+%2F+Media"
    );
    let authorization = request.header("authorization").unwrap();
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"),
        "{authorization}"
    );
    assert!(authorization.contains("/us-west-2/execute-api/aws4_request"));
    assert!(authorization.contains("SignedHeaders=") && authorization.contains("host"));
    assert!(request.header("x-amz-date").is_some());
    assert_eq!(
        request.header("x-amz-security-token"),
        Some("session-token")
    );
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert!(
        request
            .header("user-agent")
            .unwrap()
            .starts_with("heliosctl/")
    );
    assert_eq!(serde_json::from_str::<Value>(&request.body).unwrap(), body);
}

#[tokio::test]
async fn an_empty_answer_is_an_empty_object() {
    let (endpoint, _) = server(vec![(204, vec![], String::new())]).await;
    let answer = client(&endpoint)
        .call(Method::Delete, "/v1/x", &[], None, false)
        .await
        .unwrap();
    assert_eq!(answer, json!({}));
}

#[tokio::test]
async fn maps_the_apis_errors() {
    let (endpoint, _) = server(vec![(
        409,
        vec![("x-amzn-errortype", "WifiNetworkRefused:http://internal")],
        r#"{"message": "The device has no Wi-Fi adapter.", "reason": "no_adapter"}"#.into(),
    )])
    .await;
    let failure = client(&endpoint)
        .call(Method::Put, "/v1/x", &[], None, false)
        .await
        .unwrap_err();
    let Failure::Api {
        status,
        kind,
        message,
        members,
    } = failure
    else {
        panic!("not an API error");
    };
    assert_eq!((status, kind.as_str()), (409, "WifiNetworkRefused"));
    assert_eq!(message, "The device has no Wi-Fi adapter.");
    assert_eq!(members["reason"], "no_adapter");
}

#[test]
fn errors_without_a_type_are_named_by_status() {
    for (status, kind) in [
        (403, "AccessDeniedException"),
        (404, "NotFound"),
        (429, "RateLimited"),
        (500, "HTTP500"),
    ] {
        let Err(Failure::Api {
            kind: found,
            message,
            ..
        }) = parse(status, None, r#"{"Message": "nope"}"#)
        else {
            panic!("{status}");
        };
        assert_eq!(found, kind);
        assert_eq!(message, "nope");
    }
    let Err(Failure::Api { message, .. }) = parse(502, None, "") else {
        panic!("not an error");
    };
    assert_eq!(message, "no message");
    assert!(matches!(parse(200, None, "<html>"), Err(Failure::Other(_))));
}

#[tokio::test]
async fn retries_throttling_only_when_allowed() {
    let throttled: Canned = (
        429,
        vec![("x-amzn-errortype", "RateLimited")],
        r#"{"message": "slow down"}"#.into(),
    );
    let (endpoint, received) = server(vec![throttled.clone(), (200, vec![], "{}".into())]).await;
    client(&endpoint)
        .call(Method::Get, "/v1/x", &[], None, true)
        .await
        .unwrap();
    assert_eq!(received.lock().unwrap().len(), 2, "retried once");

    let (endpoint, received) = server(vec![throttled]).await;
    let failure = client(&endpoint)
        .call(Method::Post, "/v1/x", &[], None, false)
        .await
        .unwrap_err();
    assert_eq!(failure.exit_code(), 8);
    assert_eq!(
        received.lock().unwrap().len(),
        1,
        "a write without a token isn't retried"
    );
}

#[tokio::test]
async fn an_unreachable_api_is_a_network_failure() {
    // Nothing listens on port 9 of the local machine.
    let failure = client("http://127.0.0.1:9/dev")
        .call(Method::Get, "/v1/x", &[], None, false)
        .await
        .unwrap_err();
    assert!(matches!(failure, Failure::Other(_)), "{failure:?}");
    assert!(retryable(&failure));
    assert!(!retryable(&Failure::Invalid("x".into())));
}

/// A credential chain that finds nothing.
#[derive(Debug)]
struct NoCredentials;

impl ProvideCredentials for NoCredentials {
    fn provide_credentials<'a>(
        &'a self,
    ) -> aws_credential_types::provider::future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        aws_credential_types::provider::future::ProvideCredentials::ready(Err(
            aws_credential_types::provider::error::CredentialsError::not_loaded("no profile"),
        ))
    }
}

#[tokio::test]
async fn needs_credentials() {
    let none = SharedCredentialsProvider::new(NoCredentials);
    let failure = Client::new("http://127.0.0.1:9/dev", "us-west-2", none)
        .unwrap()
        .call(Method::Get, "/v1/x", &[], None, false)
        .await
        .unwrap_err();
    assert!(matches!(failure, Failure::Credentials(_)), "{failure:?}");
    assert_eq!(Method::Patch.as_str(), "PATCH");
    assert!(matches!(
        client("http://127.0.0.1:9")
            .call(Method::Get, "relative", &[], None, false)
            .await,
        Err(Failure::Invalid(_)) | Err(Failure::Other(_))
    ));
}
