use std::{env, fs, net::SocketAddr, time::Duration};

// Intentionally no Debug: this structure contains credentials.
pub struct Config {
    pub api_url: reqwest::Url,
    pub key: String,
    pub hash: String,
    pub server: String,
    pub listen: SocketAddr,
    pub timeout: Duration,
    pub cache_ttl: Duration,
    pub collect_memory_disk: bool,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let api_url = env::var("RACKNERD_API_URL")
            .unwrap_or_else(|_| "https://ctrl.racknerd.com/api/client/command.php".into());
        let api_url = validate_url(&api_url)?;
        let server = env::var("RACKNERD_SERVER").unwrap_or_else(|_| "racknerd".into());
        if server.trim().is_empty() || server.len() > 128 {
            return Err("RACKNERD_SERVER must contain 1..128 bytes".into());
        }
        let listen = env::var("RACKNERD_LISTEN_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1:9725".into())
            .parse()
            .map_err(|_| "RACKNERD_LISTEN_ADDRESS must be an IP:port")?;
        let collect_memory_disk = match env::var("RACKNERD_COLLECT_MEMORY_DISK").as_deref() {
            Err(env::VarError::NotPresent) | Ok("false") => false,
            Ok("true") => true,
            _ => return Err("RACKNERD_COLLECT_MEMORY_DISK must be true or false".into()),
        };
        Ok(Self {
            api_url,
            key: secret("RACKNERD_API_KEY")?,
            hash: secret("RACKNERD_API_HASH")?,
            server,
            listen,
            timeout: seconds("RACKNERD_TIMEOUT_SECONDS", 10, 1, 60)?,
            cache_ttl: seconds("RACKNERD_CACHE_SECONDS", 3600, 15, 3600)?,
            collect_memory_disk,
        })
    }
}

pub fn validate_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw).map_err(|_| "RACKNERD_API_URL is not a valid URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "RACKNERD_API_URL must use HTTPS without credentials, query or fragment".into(),
        );
    }
    Ok(url)
}

fn secret(name: &str) -> Result<String, String> {
    let file_name = format!("{name}_FILE");
    let value = match (env::var_os(name), env::var_os(&file_name)) {
        (Some(_), Some(_)) => return Err(format!("set only {name} or {file_name}")),
        (Some(value), None) => value
            .into_string()
            .map_err(|_| format!("{name} must be UTF-8"))?,
        (None, Some(path)) => {
            fs::read_to_string(path).map_err(|_| format!("cannot read {file_name}"))?
        }
        (None, None) => return Err(format!("set {name} or {file_name}")),
    };
    let value = value.trim().to_owned();
    if value.is_empty() || value.starts_with("replace-") || value.len() > 4096 {
        return Err(format!("{name} is empty, a placeholder, or too long"));
    }
    Ok(value)
}

fn seconds(name: &str, default: u64, min: u64, max: u64) -> Result<Duration, String> {
    let value = match env::var(name) {
        Ok(s) => s
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))?,
        Err(env::VarError::NotPresent) => default,
        Err(_) => return Err(format!("{name} must be UTF-8")),
    };
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be in {min}..={max}"));
    }
    Ok(Duration::from_secs(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_must_keep_secrets_out_of_url_and_use_tls() {
        for url in [
            "http://example.com/api",
            "https://u:p@example.com/api",
            "https://example.com/api?key=x",
            "https://example.com/api#x",
        ] {
            assert!(validate_url(url).is_err());
        }
        assert!(validate_url("https://ctrl.racknerd.com/api/client/command.php").is_ok());
    }
}
