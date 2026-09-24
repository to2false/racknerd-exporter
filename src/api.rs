use crate::config::Config;
use serde::Deserialize;
use std::{fmt, time::Duration};

pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub limit: f64,
    pub used: f64,
    pub free: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Info {
    pub online: Option<bool>,
    pub disabled: Option<bool>,
    pub bandwidth: Option<Usage>,
    pub memory: Option<Usage>,
    pub disk: Option<Usage>,
}

// Errors intentionally contain neither upstream response bodies nor reqwest URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    Transport,
    Timeout,
    Http(u16),
    TooLarge,
    InvalidResponse,
    Rejected,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => f.write_str("API connection or TLS error"),
            Self::Timeout => f.write_str("API request timed out"),
            Self::Http(status) => write!(f, "API HTTP status {status}"),
            Self::TooLarge => f.write_str("API response exceeds 1 MiB"),
            Self::InvalidResponse => f.write_str("invalid API response"),
            Self::Rejected => {
                f.write_str("API rejected the request; check credentials and API access")
            }
        }
    }
}

impl std::error::Error for ApiError {}

pub struct ApiClient {
    client: reqwest::Client,
    config: Config,
}

impl ApiClient {
    pub fn new(config: Config) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .connect_timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("racknerd-exporter/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client, config })
    }

    pub fn server(&self) -> &str {
        &self.config.server
    }
    pub fn cache_ttl(&self) -> Duration {
        self.config.cache_ttl
    }

    pub async fn fetch(&self) -> Result<Info, ApiError> {
        let extra = if self.config.collect_memory_disk {
            "true"
        } else {
            "false"
        };
        let mut response = self
            .client
            .post(self.config.api_url.clone())
            .form(&[
                ("key", self.config.key.as_str()),
                ("hash", self.config.hash.as_str()),
                ("action", "info"),
                ("bw", "true"),
                ("status", "true"),
                ("hdd", extra),
                ("mem", extra),
            ])
            .send()
            .await
            .map_err(transport_error)?;
        if !response.status().is_success() {
            return Err(ApiError::Http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ApiError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(ApiError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let text = std::str::from_utf8(&body).map_err(|_| ApiError::InvalidResponse)?;
        parse_info(text, self.config.collect_memory_disk)
    }
}

fn transport_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Transport
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Response {
    status: String,
    statusmsg: String,
    vmstat: String,
    bw: String,
    mem: String,
    hdd: String,
}

pub fn parse_info(text: &str, collect_memory_disk: bool) -> Result<Info, ApiError> {
    if text.len() > MAX_RESPONSE_BYTES {
        return Err(ApiError::TooLarge);
    }
    let mut text = text.trim_start_matches('\u{feff}').trim();
    if text.starts_with("<?xml ") {
        text = text
            .split_once("?>")
            .ok_or(ApiError::InvalidResponse)?
            .1
            .trim();
    }
    if text.contains("<!DOCTYPE") || text.contains("<!ENTITY") {
        return Err(ApiError::InvalidResponse);
    }
    // RackNerd ctrl uses a <ctrl> root; legacy SolusVM uses adjacent elements.
    if let Some(inner) = text.strip_prefix("<ctrl>") {
        text = inner
            .strip_suffix("</ctrl>")
            .ok_or(ApiError::InvalidResponse)?;
    }
    let wrapped = format!("<response>{text}</response>");
    let raw: Response = quick_xml::de::from_str(&wrapped).map_err(|_| ApiError::InvalidResponse)?;
    match raw.status.trim() {
        "success" => {}
        "error" | "failed" => return Err(ApiError::Rejected),
        _ => return Err(ApiError::InvalidResponse),
    }
    let state = if raw.vmstat.trim().is_empty() {
        &raw.statusmsg
    } else {
        &raw.vmstat
    };
    let (online, disabled) = match state.trim().to_ascii_lowercase().as_str() {
        "online" => (Some(true), Some(false)),
        "offline" => (Some(false), Some(false)),
        "disabled" => (Some(false), Some(true)),
        _ => (None, None),
    };
    Ok(Info {
        online,
        disabled,
        bandwidth: parse_usage(&raw.bw)?,
        memory: if collect_memory_disk {
            parse_usage(&raw.mem)?
        } else {
            None
        },
        disk: if collect_memory_disk {
            parse_usage(&raw.hdd)?
        } else {
            None
        },
    })
}

fn parse_usage(raw: &str) -> Result<Option<Usage>, ApiError> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let fields = raw
        .split(',')
        .map(str::trim)
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ApiError::InvalidResponse)?;
    if fields.len() != 4
        || fields.iter().any(|x| !x.is_finite())
        || fields[0] < 0.0
        || fields[1] < 0.0
        || fields[3] < 0.0
    {
        return Err(ApiError::InvalidResponse);
    }
    // All-zero values mean no usable quota/usage data; never publish fake zero usage.
    if fields.iter().all(|x| *x == 0.0) {
        return Ok(None);
    }
    Ok(Some(Usage {
        limit: fields[0],
        used: fields[1],
        free: fields[2],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_ctrl_response() {
        let info = parse_info(include_str!("../tests/fixtures/ctrl-info.xml"), true).unwrap();
        assert_eq!(info.online, Some(true));
        assert_eq!(info.disabled, Some(false));
        assert_eq!(info.bandwidth.unwrap().limit, 1_073_741_824_000.0);
        assert_eq!(info.memory.unwrap().used, 268_435_456.0);
        assert_eq!(info.disk.unwrap().used, 5_368_709_120.0);
    }

    #[test]
    fn parses_xml_fragments_and_optional_resources() {
        let info = parse_info("<?xml version=\"1.0\"?><status>success</status><vmstat>online</vmstat><bw>1000,250,750,25</bw><mem>0,0,0,0</mem><hdd>100,20,80,20</hdd>", true).unwrap();
        assert_eq!(info.online, Some(true));
        assert_eq!(info.bandwidth.unwrap().used, 250.0);
        assert!(info.memory.is_none());
        assert_eq!(info.disk.unwrap().limit, 100.0);
    }

    #[test]
    fn offline_unknown_and_missing_are_distinct() {
        assert_eq!(
            parse_info(
                "<status>success</status><statusmsg>offline</statusmsg>",
                false
            )
            .unwrap()
            .online,
            Some(false)
        );
        let info = parse_info("<status>success</status><vmstat>unknown</vmstat>", false).unwrap();
        assert_eq!(info.online, None);
        assert_eq!(info.disabled, None);
        assert_eq!(info.bandwidth, None);
    }

    #[test]
    fn recognizes_disabled_state_documented_by_racknerd() {
        let info = parse_info(
            "<ctrl><status>success</status><vmstat>disabled</vmstat></ctrl>",
            false,
        )
        .unwrap();
        assert_eq!(info.online, Some(false));
        assert_eq!(info.disabled, Some(true));
        let online = parse_info(
            "<ctrl><status>success</status><vmstat>online</vmstat></ctrl>",
            false,
        )
        .unwrap();
        assert_eq!(online.disabled, Some(false));
    }

    #[test]
    fn handles_observed_racknerd_ctrl_envelope() {
        let rejected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ctrl><status>error</status><statusmsg>Invalid key or hash</statusmsg></ctrl>";
        assert_eq!(parse_info(rejected, false), Err(ApiError::Rejected));
        assert!(parse_info("<ctrl><status>success</status></ctrl><broken>", false).is_err());
        assert_eq!(
            parse_info(
                "<ctrl><status>success</status><vmstat>online</vmstat><bw>100,25,75,25</bw></ctrl>",
                false
            )
            .unwrap()
            .online,
            Some(true)
        );
    }

    #[test]
    fn rejects_errors_html_malformed_xml_and_nonfinite_values() {
        for text in [
            "<status>error</status><statusmsg>secret</statusmsg>",
            "<html>Login</html>",
            "<status>success",
            "<status>success</status><bw>100,NaN,50,50</bw>",
            "<status>success</status><bw>1,2,3</bw>",
            "<!DOCTYPE x><status>success</status>",
            "<status>success</status><status>error</status>",
        ] {
            assert!(parse_info(text, true).is_err(), "accepted {text}");
        }
    }

    #[test]
    fn allows_overage_and_zero_limit_without_dividing_by_zero() {
        let over = parse_usage("100,120,-20,120").unwrap().unwrap();
        assert_eq!(over.free, -20.0);
        assert_eq!(parse_usage("0,120,0,0").unwrap().unwrap().limit, 0.0);
    }
}
