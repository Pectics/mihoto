use anyhow::{anyhow, bail, Result};
use base64::{prelude::BASE64_STANDARD, Engine};
use futures_util::StreamExt;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, USER_AGENT},
    Client,
};
use std::{collections::HashMap, fs, path::Path, time::Duration};

use crate::{
    config::ProfileSource,
    utils::{redact_sensitive, resolve_download_url, write_private_file},
};

pub const MAX_SUBSCRIPTION_BYTES: usize = 16 * 1024 * 1024;

pub fn classify_subscription_response(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() > MAX_SUBSCRIPTION_BYTES {
        bail!("subscription response exceeds 16 MiB limit");
    }

    let text = std::str::from_utf8(bytes)
        .map_err(|_| anyhow!("subscription response is not valid UTF-8"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        bail!("subscription response is empty");
    }

    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        bail!("subscription response is HTML, not a Mihomo YAML config");
    }

    if looks_like_v2ray_json(trimmed) {
        bail!("subscription response appears to be V2Ray JSON, not Mihomo YAML");
    }

    if let Ok(decoded) = BASE64_STANDARD.decode(trimmed.as_bytes()) {
        if validate_mihomo_yaml(&decoded).is_ok() {
            return Ok(decoded);
        }
    }

    validate_mihomo_yaml(bytes)?;
    Ok(bytes.to_vec())
}

pub async fn fetch_profile_source(
    client: &Client,
    source: &ProfileSource,
    user_agent: &str,
    headers: &HashMap<String, String>,
    dest: &Path,
) -> Result<()> {
    let bytes = match source {
        ProfileSource::Url { url } => fetch_url_source(client, url, user_agent, headers).await?,
        ProfileSource::File { path } | ProfileSource::Existing { path } => {
            fs::read(shellexpand::tilde(path).as_ref())?
        }
    };
    let classified = classify_subscription_response(&bytes)?;
    write_private_file(dest, &classified)
}

async fn fetch_url_source(
    client: &Client,
    url: &str,
    user_agent: &str,
    headers: &HashMap<String, String>,
) -> Result<Vec<u8>> {
    let resolved_url = resolve_download_url(url);
    let mut request_headers = HeaderMap::new();
    request_headers.insert(USER_AGENT, HeaderValue::from_str(user_agent)?);
    for (key, value) in headers {
        let name = HeaderName::from_bytes(key.as_bytes())
            .map_err(|err| anyhow!("invalid profile header name `{key}`: {err}"))?;
        let value = HeaderValue::from_str(value)
            .map_err(|err| anyhow!("invalid value for profile header `{key}`: {err}"))?;
        request_headers.insert(name, value);
    }

    let response = client
        .get(resolved_url.as_ref())
        .headers(request_headers)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|err| {
            anyhow!(
                "failed to GET from '{}': {err}",
                redact_sensitive(resolved_url.as_ref())
            )
        })?;
    response.error_for_status_ref()?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_SUBSCRIPTION_BYTES as u64)
    {
        bail!("subscription response exceeds 16 MiB limit");
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > MAX_SUBSCRIPTION_BYTES {
            bail!("subscription response exceeds 16 MiB limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn looks_like_v2ray_json(trimmed: &str) -> bool {
    trimmed.starts_with('{')
        && (trimmed.contains("\"v\"")
            || trimmed.contains("\"ps\"")
            || trimmed.contains("\"add\"")
            || trimmed.contains("\"outbounds\""))
}

fn validate_mihomo_yaml(bytes: &[u8]) -> Result<()> {
    let value: serde_yaml::Value = serde_yaml::from_slice(bytes)
        .map_err(|err| anyhow!("subscription response is not valid Mihomo YAML: {err}"))?;
    match value {
        serde_yaml::Value::Mapping(_) => Ok(()),
        _ => bail!("subscription response is not a Mihomo YAML mapping"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProfileSource;
    use base64::{prelude::BASE64_STANDARD, Engine};
    use reqwest::Client;
    use std::{collections::HashMap, fs};
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[test]
    fn classifies_plain_mihomo_yaml() -> anyhow::Result<()> {
        let classified = classify_subscription_response(b"proxies: []\nrules: []\n")?;

        assert_eq!(String::from_utf8(classified)?, "proxies: []\nrules: []\n");
        Ok(())
    }

    #[test]
    fn decodes_base64_mihomo_yaml() -> anyhow::Result<()> {
        let encoded = BASE64_STANDARD.encode("proxies: []\nproxy-groups: []\n");

        let classified = classify_subscription_response(encoded.as_bytes())?;

        assert_eq!(
            String::from_utf8(classified)?,
            "proxies: []\nproxy-groups: []\n"
        );
        Ok(())
    }

    #[test]
    fn rejects_empty_html_v2ray_json_and_oversized_responses() {
        assert!(classify_subscription_response(b"   ")
            .unwrap_err()
            .to_string()
            .contains("empty"));
        assert!(
            classify_subscription_response(b"<html><body>login</body></html>")
                .unwrap_err()
                .to_string()
                .contains("HTML")
        );
        assert!(classify_subscription_response(br#"{"v":"2","ps":"node"}"#)
            .unwrap_err()
            .to_string()
            .contains("V2Ray JSON"));
        assert!(
            classify_subscription_response(&vec![b'a'; MAX_SUBSCRIPTION_BYTES + 1])
                .unwrap_err()
                .to_string()
                .contains("16 MiB")
        );
    }

    #[tokio::test]
    async fn url_adapter_sends_profile_headers_and_writes_classified_yaml() -> anyhow::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nproxies: []\n";
            socket.write_all(response).await.unwrap();
            request
        });

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        let dir = tempdir()?;
        let dest = dir.path().join("source.yaml");

        fetch_profile_source(
            &Client::new(),
            &ProfileSource::Url {
                url: format!("http://{addr}/sub"),
            },
            "mihoro-test",
            &headers,
            &dest,
        )
        .await?;

        let request = server.await?;
        let request = request.to_ascii_lowercase();
        assert!(request.contains("user-agent: mihoro-test"));
        assert!(request.contains("authorization: bearer token"));
        assert_eq!(fs::read_to_string(dest)?, "proxies: []\n");
        Ok(())
    }
}
