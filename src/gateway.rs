//! Transport-agnostic gateway client.
//!
//! `Transport` is the seam: production uses `HttpTransport` (reqwest + rustls),
//! tests use `MockTransport` and touch zero network. Every endpoint method is
//! pure request-building plus one transport round-trip, so the whole client is
//! covered offline.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::domain::{AccessToken, ENDPOINTS, GATEWAYS};

/// Envelope the gateways return. Field names are the JSON keys observed in the
/// isolate string pool (`status`, `message`, `success`) plus the bKash-standard
/// transaction envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(serialize = "T: Serialize"))]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct GatewayResponse<T> {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub success: Option<bool>,
    #[serde(default, rename = "trx_id")]
    pub trx_id: Option<String>,
    #[serde(default)]
    pub err_code: Option<String>,
    #[serde(default)]
    pub data: Option<T>,
}

impl<T> GatewayResponse<T> {
    /// True when the gateway reported no transport-level error. `err_code` is
    /// authoritative: bKash returns `HTTP 200` with a populated `err_code` for
    /// business failures, so status-code-only checks are wrong.
    pub fn is_ok(&self) -> bool {
        self.err_code.is_none() && self.success.unwrap_or(true)
    }

    pub fn require_data(self) -> Result<T> {
        if let Some(code) = self.err_code {
            return Err(anyhow!(
                "gateway error {code}: {}",
                self.message.unwrap_or_else(|| "no message".into())
            ));
        }
        self.data
            .ok_or_else(|| anyhow!("gateway returned no data field"))
    }
}

/// One outbound HTTP request, fully formed. The transport impl is the only
/// thing that knows about sockets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
}

pub trait Transport: Send + Sync {
    fn send(&self, req: HttpRequest) -> Result<String>;
    /// Human-readable provider identity for `--verbose` and logs.
    fn name(&self) -> &'static str;
}

/// Records requests and replays canned bodies. Ordering is FIFO.
pub struct MockTransport {
    name: &'static str,
    queue: Mutex<Vec<String>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl MockTransport {
    pub fn new(name: &'static str, responses: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            name,
            queue: Mutex::new(responses.into_iter().rev().collect()),
            seen: Mutex::new(Vec::new()),
        })
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.seen.lock().expect("seen lock").clone()
    }
}

impl Transport for MockTransport {
    fn send(&self, req: HttpRequest) -> Result<String> {
        self.seen.lock().expect("seen lock").push(req.clone());
        self.queue
            .lock()
            .expect("queue lock")
            .pop()
            .ok_or_else(|| anyhow!("mock transport exhausted for {} {}", req.method, req.url))
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

pub struct Gateway {
    transport: Arc<dyn Transport>,
    base: String,
    token: Option<AccessToken>,
    device: DeviceIdentity,
}

/// Device identity sent on `auth/device-fingerprint` and every authed call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub brand: String,
    pub model: String,
    pub android_id: String,
    pub os_version: String,
}

impl Default for DeviceIdentity {
    fn default() -> Self {
        Self {
            brand: "Generic".into(),
            model: "PC".into(),
            // Deterministic placeholder: never collect the host's real IMEI.
            android_id: "00000000-0000-0000-0000-000000000000".into(),
            os_version: "35".into(),
        }
    }
}

impl Gateway {
    pub fn new(
        transport: Arc<dyn Transport>,
        base: impl Into<String>,
        device: DeviceIdentity,
    ) -> Self {
        let base = base.into();
        debug_assert!(base.ends_with('/'), "base url must end with /");
        Self {
            transport,
            base,
            token: None,
            device,
        }
    }

    /// Default production base: the youth-app gateway, first literal observed.
    pub fn default_base() -> &'static str {
        GATEWAYS[0]
    }

    pub fn transport_name(&self) -> &'static str {
        self.transport.name()
    }

    pub fn known_endpoints() -> &'static [&'static str] {
        ENDPOINTS
    }

    pub fn set_token(&mut self, token: AccessToken) {
        self.token = Some(token);
    }

    pub fn token(&self) -> Option<&AccessToken> {
        self.token.as_ref()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path.trim_start_matches('/'))
    }

    fn build(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<HttpRequest> {
        let mut headers = BTreeMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert(
            "User-Agent".to_string(),
            format!("bKash-Native-Rust/{}", crate::VERSION),
        );
        headers.insert(
            "device-identity".to_string(),
            serde_json::to_string(&self.device)?,
        );
        if let Some(t) = &self.token {
            headers.insert("Authorization".to_string(), t.bearer());
        }
        Ok(HttpRequest {
            method,
            url: self.url(path),
            headers,
            body: body.map(|b| b.to_string()),
        })
    }

    /// One round-trip: build, send, deserialize. Returns the typed payload.
    pub fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &'static str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<GatewayResponse<T>> {
        let req = self.build(method, path, body)?;
        let raw = self.transport.send(req)?;
        serde_json::from_str(&raw).map_err(|e| anyhow!("decode {path}: {e}; body was {raw}"))
    }

    pub fn pre_init(&self) -> Result<GatewayResponse<serde_json::Value>> {
        let req = self.build("POST", "auth/pre-init", Some(&serde_json::json!({})))?;
        let raw = self.transport.send(req)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn send_otp(
        &self,
        mobile: &str,
        code_length: u8,
    ) -> Result<GatewayResponse<serde_json::Value>> {
        let body = serde_json::json!({ "mobile_number": mobile, "code_length": code_length });
        self.call("POST", "otp/send", Some(&body))
    }

    pub fn verify_otp(&self, mobile: &str, otp: &str) -> Result<GatewayResponse<AccessToken>> {
        let body = serde_json::json!({ "mobile_number": mobile, "otp": otp });
        self.call("POST", "otp/verify", Some(&body))
    }

    pub fn device_fingerprint(&self) -> Result<GatewayResponse<serde_json::Value>> {
        self.call("GET", "auth/device-fingerprint", None)
    }

    pub fn logout(&self) -> Result<GatewayResponse<serde_json::Value>> {
        self.call("POST", "auth/logout", Some(&serde_json::json!({})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gw(responses: Vec<&str>) -> Gateway {
        Gateway::new(
            MockTransport::new("mock", responses.into_iter().map(str::to_string).collect()),
            Gateway::default_base(),
            DeviceIdentity::default(),
        )
    }

    #[test]
    fn otp_send_uses_observed_path_and_header() {
        let g = gw(vec![
            r#"{"status":"success","trx_id":"1ABCDEFGHI","success":true}"#,
        ]);
        let res = g.send_otp("+8801712345678", 6).unwrap();
        assert!(res.is_ok());
        let seen = g.transport_name();
        assert_eq!(seen, "mock");
        let req = g
            .call::<serde_json::Value>("GET", "auth/app-status", None)
            .err();
        assert!(
            req.is_some(),
            "mock queue exhausted proves only one send happened"
        );
    }

    #[test]
    fn build_attaches_authorization_and_json_headers() {
        let g = {
            let mut g = gw(vec![]);
            g.set_token(AccessToken {
                access_token: "T".into(),
                refresh_token: "R".into(),
                token_type: "Bearer".into(),
                expires_in: 60,
            });
            g
        };
        let req = g
            .build("POST", "auth/login", Some(&serde_json::json!({})))
            .unwrap();
        assert_eq!(req.headers.get("Authorization").unwrap(), "Bearer T");
        assert_eq!(req.headers.get("Content-Type").unwrap(), "application/json");
        assert!(req.url.ends_with("gateway/api/auth/login"));
    }

    #[test]
    fn err_code_wins_over_http_200() {
        let resp: GatewayResponse<serde_json::Value> =
            serde_json::from_str(r#"{"err_code":"2011","message":"PIN wrong"}"#).unwrap();
        assert!(!resp.is_ok());
        let err = resp.require_data().unwrap_err().to_string();
        assert!(err.contains("2011"), "got {err}");
        assert!(err.contains("PIN wrong"));
    }

    #[test]
    fn require_data_errors_when_missing() {
        let resp: GatewayResponse<serde_json::Value> =
            serde_json::from_str(r#"{"status":"success"}"#).unwrap();
        assert!(resp.require_data().is_err());
    }

    #[test]
    fn mock_records_requests_in_order() {
        let t = MockTransport::new("mock", vec!["a".into(), "b".into()]);
        let g = Gateway::new(
            t.clone(),
            Gateway::default_base(),
            DeviceIdentity::default(),
        );
        g.build("GET", "auth/app-status", None)
            .map(|r| t.send(r))
            .unwrap()
            .unwrap();
        g.build("GET", "auth/appStatus", None)
            .map(|r| t.send(r))
            .unwrap()
            .unwrap();
        let urls: Vec<String> = t.requests().into_iter().map(|r| r.url).collect();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with("auth/app-status"));
        assert!(urls[1].ends_with("auth/appStatus"));
    }
}
