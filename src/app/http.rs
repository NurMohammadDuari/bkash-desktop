//! The one piece that knows about sockets.
//!
//! Kept in the library (not in a binary) so the desktop app and the CLI share
//! exactly one HTTP implementation. TLS is rustls; no system OpenSSL needed.

use anyhow::{bail, Result};
use crate::gateway::{HttpRequest, Transport};

pub struct HttpTransport {
    pub client: reqwest::blocking::Client,
}

impl HttpTransport {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .user_agent(format!("bKash-Native-Rust/{}", crate::VERSION))
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
        })
    }
}

impl Transport for HttpTransport {
    fn send(&self, req: HttpRequest) -> Result<String> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes())?;
        let mut builder = self.client.request(method, &req.url);
        for (k, v) in &req.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }
        let resp = builder.send()?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() {
            bail!("HTTP {status} from {}: {}", req.url, truncate(&text, 300));
        }
        Ok(text)
    }

    fn name(&self) -> &'static str {
        "reqwest-rustls"
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}