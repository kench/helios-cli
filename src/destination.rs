//! SRT and SRTLA destination URLs (the contract's `DestinationUrl`, HLD 7.3.2), checked with the
//! service's own rules (helios-cloud's `Destinations.java`) so `--dry-run` refuses exactly what
//! the API would: `srt://` or `srtla://`, a host name or IP address, a port, and only `streamid`,
//! `latency` (milliseconds) and `passphrase`, each at most once.

/// A checked destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// `srtla` rather than `srt`.
    pub bonded: bool,
    /// The host name or IP address (an IPv6 address without brackets).
    pub host: String,
    /// The port.
    pub port: u16,
    /// The SRT stream ID.
    pub stream_id: Option<String>,
    /// The SRT latency in milliseconds.
    pub latency_ms: Option<u32>,
    /// The SRT passphrase.
    pub passphrase: Option<String>,
}

/// Why a destination was refused: the part that's wrong, as `InvalidDestination.part` names it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Refused {
    /// `scheme`, `host`, `port`, `fragment`, a parameter's name, or `parameter`.
    pub part: String,
    /// For people.
    pub message: String,
}

fn refused(part: &str, message: &str) -> Refused {
    Refused {
        part: part.to_owned(),
        message: message.to_owned(),
    }
}

const PARAMETERS: [&str; 3] = ["streamid", "latency", "passphrase"];

impl Destination {
    /// The URL in its normal form: the parameters in a fixed order, nothing percent-encoded.
    #[must_use]
    pub fn url(&self) -> String {
        let mut query = Vec::new();
        if let Some(stream_id) = &self.stream_id {
            query.push(format!("streamid={stream_id}"));
        }
        if let Some(latency) = self.latency_ms {
            query.push(format!("latency={latency}"));
        }
        if let Some(passphrase) = &self.passphrase {
            query.push(format!("passphrase={passphrase}"));
        }
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!(
            "{}://{host}:{}{}{}",
            if self.bonded { "srtla" } else { "srt" },
            self.port,
            if query.is_empty() { "" } else { "?" },
            query.join("&")
        )
    }
}

/// Parses and checks `url`.
///
/// # Errors
///
/// [`Refused`], naming the part that's wrong.
pub fn parse(url: &str) -> Result<Destination, Refused> {
    let (bonded, rest) = if let Some(rest) = url.strip_prefix("srt://") {
        (false, rest)
    } else if let Some(rest) = url.strip_prefix("srtla://") {
        (true, rest)
    } else {
        return Err(refused(
            "scheme",
            "The destination must be an srt:// or srtla:// URL.",
        ));
    };
    if rest.contains('#') {
        return Err(refused(
            "fragment",
            "The destination can't have a #fragment.",
        ));
    }
    let (authority, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, port) = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some((host, after)) = bracketed.split_once(']') else {
            return Err(refused(
                "host",
                "An IPv6 address needs brackets, then :port.",
            ));
        };
        let Some(port) = after.strip_prefix(':') else {
            return Err(refused(
                "host",
                "An IPv6 address needs brackets, then :port.",
            ));
        };
        let ipv6 = (2..=45).contains(&host.len())
            && host
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
        if !ipv6 {
            return Err(refused("host", "Not an IPv6 address."));
        }
        (host, port)
    } else {
        let Some((host, port)) = authority.rsplit_once(':') else {
            return Err(refused("port", "The destination needs a port: host:port."));
        };
        let name = (1..=253).contains(&host.len())
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if !name {
            return Err(refused(
                "host",
                "The host must be a host name or IP address.",
            ));
        }
        (host, port)
    };
    let port = digits(port)
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port >= 1)
        .ok_or_else(|| refused("port", "The port must be 1 to 65535."))?;

    let mut parameters: Vec<(&str, String)> = Vec::new();
    if !query.is_empty() {
        for pair in query.split('&') {
            let (name, value) = match pair.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (pair, None),
            };
            if !PARAMETERS.contains(&name) {
                let part = if name.is_empty() { "parameter" } else { name };
                return Err(refused(
                    part,
                    "Only streamid, latency and passphrase are allowed.",
                ));
            }
            let Some(value) = value else {
                return Err(refused(name, &format!("{name} needs a value.")));
            };
            if parameters.iter().any(|(seen, _)| *seen == name) {
                return Err(refused(name, &format!("{name} is given more than once.")));
            }
            parameters.push((name, decode(value, name)?));
        }
    }
    let get = |name: &str| {
        parameters
            .iter()
            .find(|(seen, _)| *seen == name)
            .map(|(_, value)| value.clone())
    };
    let stream_id = get("streamid");
    if let Some(stream_id) = &stream_id {
        let ok = (1..=512).contains(&stream_id.len())
            && stream_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "/_.:=,-".contains(c));
        if !ok {
            return Err(refused(
                "streamid",
                "streamid must be up to 512 letters, digits and / _ . - : = , characters.",
            ));
        }
    }
    let latency_ms = match get("latency") {
        None => None,
        Some(latency) => Some(
            digits(&latency)
                .filter(|ms| (20..=30_000).contains(ms))
                .ok_or_else(|| refused("latency", "latency must be 20 to 30000 milliseconds."))?,
        ),
    };
    let passphrase = get("passphrase");
    if let Some(passphrase) = &passphrase {
        let ok = (10..=79).contains(&passphrase.len())
            && passphrase
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._~!$'()*+,;:@-".contains(c));
        if !ok {
            return Err(refused(
                "passphrase",
                "passphrase must be 10 to 79 letters, digits and - . _ ~ ! $ ' ( ) * + , ; : @ \
                 characters.",
            ));
        }
    }
    Ok(Destination {
        bonded,
        host: host.to_owned(),
        port,
        stream_id,
        latency_ms,
        passphrase,
    })
}

/// One to five ASCII digits.
fn digits(text: &str) -> Option<u32> {
    ((1..=5).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Percent-decoding as in RFC 3986: `+` is a plus sign, not a space.
fn decode(value: &str, name: &str) -> Result<String, Refused> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'%' {
            let hex = |b: Option<&u8>| b.and_then(|b| char::from(*b).to_digit(16));
            let (Some(high), Some(low)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) else {
                return Err(refused(name, "A % escape needs two hex digits."));
            };
            out.push(u8::try_from(high * 16 + low).unwrap_or(0));
            i += 3;
        } else if !(0x21..=0x7e).contains(&c) {
            return Err(refused(
                name,
                &format!("{name} has a character that must be percent-encoded."),
            ));
        } else {
            out.push(c);
            i += 1;
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The same cases as helios-cloud's DestinationsTest, so both refuse the same URLs.

    #[test]
    fn reads_an_srt_url_and_writes_its_normal_form() {
        let d = parse(
            "srt://ingest.example.com:4001?passphrase=not-a-secret-123&latency=2000\
             &streamid=publish/live/pocket",
        )
        .unwrap();
        assert!(!d.bonded);
        assert_eq!(d.host, "ingest.example.com");
        assert_eq!(d.port, 4001);
        assert_eq!(d.stream_id.as_deref(), Some("publish/live/pocket"));
        assert_eq!(d.latency_ms, Some(2000));
        assert_eq!(d.passphrase.as_deref(), Some("not-a-secret-123"));
        assert_eq!(
            d.url(),
            "srt://ingest.example.com:4001?streamid=publish/live/pocket&latency=2000\
             &passphrase=not-a-secret-123"
        );
    }

    #[test]
    fn reads_srtla_ipv6_and_percent_escapes() {
        let bonded = parse("srtla://10.0.0.1:5000").unwrap();
        assert!(bonded.bonded);
        assert_eq!(bonded.stream_id, None);
        assert_eq!(bonded.latency_ms, None);
        assert_eq!(bonded.url(), "srtla://10.0.0.1:5000");
        let v6 =
            parse("srt://[2001:db8::1]:4001?streamid=publish%2Flive%2Fa&passphrase=a+b%21cdefghij")
                .unwrap();
        assert_eq!(v6.host, "2001:db8::1");
        assert_eq!(v6.stream_id.as_deref(), Some("publish/live/a"));
        assert_eq!(
            v6.passphrase.as_deref(),
            Some("a+b!cdefghij"),
            "+ is a plus sign"
        );
        assert_eq!(
            v6.url(),
            "srt://[2001:db8::1]:4001?streamid=publish/live/a&passphrase=a+b!cdefghij"
        );
    }

    #[test]
    fn refuses_anything_else_and_names_the_part() {
        for (url, part) in [
            ("rtmp://host:1935/live", "scheme"),
            ("srt://host", "port"),
            ("srt://host:0", "port"),
            ("srt://host:70000", "port"),
            ("srt://host:12a", "port"),
            ("srt://ho st:1", "host"),
            ("srt://:1", "host"),
            ("srt://[2001:db8::1:4001", "host"),
            ("srt://[zz]:4001", "host"),
            ("srt://host:1?mode=caller", "mode"),
            ("srt://host:1?=x", "parameter"),
            ("srt://host:1?latency", "latency"),
            ("srt://host:1?latency=10", "latency"),
            ("srt://host:1?latency=30001", "latency"),
            ("srt://host:1?latency=2000&latency=3000", "latency"),
            ("srt://host:1?passphrase=short", "passphrase"),
            ("srt://host:1?passphrase=has%20space123", "passphrase"),
            ("srt://host:1?streamid=a%26b", "streamid"),
            ("srt://host:1?streamid=a%2", "streamid"),
            ("srt://host:1?streamid=a%zz", "streamid"),
            ("srt://host:1?streamid=caf\u{e9}", "streamid"),
            ("srt://host:1#top", "fragment"),
            ("srt://[2001:db8::1]4001", "host"),
        ] {
            assert_eq!(parse(url).unwrap_err().part, part, "{url}");
        }
        assert_eq!(
            parse("srt://h:1?latency=10").unwrap_err().to_string(),
            "latency must be 20 to 30000 milliseconds."
        );
    }
}
