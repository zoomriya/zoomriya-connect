//! Talking to the control plane: registering, the name's records, the
//! certificate check, the outside check, guests' relay credentials.
//! Every request is signed with the key the server knows for it.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde::{Serialize, de::DeserializeOwned};
use zoomriya_keys::{REQUEST_SIGNATURE_HEADER, SecretKey, sign_request};
use zoomriya_proto::{
    AcmeRequest, AcmeTarget, DoorCheck, DoorRequest, ErrorBody, HomeAddrs, PROTOCOL_VERSION,
    RegisterRequest, RegisterResponse, RelayUse, TurnCredentials, VERSION_HEADER, WellKnown,
};

/// A refusal from the server, with its words for Box Desktop.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ServerSaid {
    /// HTTP status.
    pub status: u16,
    /// Stable code, such as `name_taken`.
    pub code: String,
    /// Plain words.
    pub message: String,
}

/// The control plane.
#[derive(Debug, Clone)]
pub struct Server {
    base: String,
    http: reqwest::Client,
}

impl Server {
    /// The server at `base`, such as `https://connect.zoomriya.net`.
    pub fn new(base: &str) -> Result<Self> {
        Ok(Self {
            base: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("zoomriya-connectd/", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    /// Its address.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// `/.well-known/zoomriya-connect.json`: everything else from one address.
    pub async fn well_known(&self) -> Result<WellKnown> {
        let res = self
            .http
            .get(format!("{}/.well-known/zoomriya-connect.json", self.base))
            .send()
            .await
            .context("asking the ZOOMRIYA Connect server")?;
        anyhow::ensure!(
            res.status().is_success(),
            "the server said {}",
            res.status()
        );
        Ok(res.json().await?)
    }

    async fn signed<B: Serialize, T: DeserializeOwned>(
        &self,
        key: &SecretKey,
        method: reqwest::Method,
        path: &str,
        body: &B,
    ) -> Result<Option<T>> {
        let body = serde_json::to_vec(body)?;
        let sig = sign_request(key, method.as_str(), path, crate::now(), &body);
        let res = self
            .http
            .request(method, format!("{}{path}", self.base))
            .header(REQUEST_SIGNATURE_HEADER, sig)
            .header(VERSION_HEADER, PROTOCOL_VERSION.to_string())
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .context("asking the ZOOMRIYA Connect server")?;
        let status = res.status().as_u16();
        if res.status().is_success() {
            if status == 204 {
                return Ok(None);
            }
            return Ok(Some(res.json().await?));
        }
        let said = res.json::<ErrorBody>().await.unwrap_or_else(|_| ErrorBody {
            code: "server".into(),
            message: format!("The ZOOMRIYA Connect server answered {status}."),
        });
        Err(anyhow!(ServerSaid {
            status,
            code: said.code,
            message: said.message,
        }))
    }

    /// Registers the box (or again, with a new DNS update key).
    pub async fn register(
        &self,
        box_key: &SecretKey,
        req: &RegisterRequest,
    ) -> Result<RegisterResponse> {
        self.signed(box_key, reqwest::Method::POST, "/v1/boxes", req)
            .await?
            .context("the server sent no answer")
    }

    /// Where the name points.
    pub async fn set_home(&self, dns_key: &SecretKey, name: &str, home: &HomeAddrs) -> Result<()> {
        self.signed::<_, serde_json::Value>(
            dns_key,
            reqwest::Method::PUT,
            &format!("/v1/boxes/{name}/home"),
            home,
        )
        .await
        .map(|_| ())
    }

    /// The certificate check's records; empty removes them.
    pub async fn set_acme(
        &self,
        dns_key: &SecretKey,
        name: &str,
        target: AcmeTarget,
        values: Vec<String>,
    ) -> Result<()> {
        self.signed::<_, serde_json::Value>(
            dns_key,
            reqwest::Method::PUT,
            &format!("/v1/boxes/{name}/acme"),
            &AcmeRequest { target, values },
        )
        .await
        .map(|_| ())
    }

    /// Opens the guests' door in DNS at the address this request comes from.
    #[allow(dead_code, reason = "the guests' door comes next")]
    pub async fn set_door(
        &self,
        dns_key: &SecretKey,
        name: &str,
        door: &DoorRequest,
    ) -> Result<()> {
        self.signed::<_, serde_json::Value>(
            dns_key,
            reqwest::Method::PUT,
            &format!("/v1/boxes/{name}/door"),
            door,
        )
        .await
        .map(|_| ())
    }

    /// Closes the guests' door in DNS.
    #[allow(dead_code, reason = "the guests' door comes next")]
    pub async fn close_door(&self, dns_key: &SecretKey, name: &str) -> Result<()> {
        self.signed::<_, serde_json::Value>(
            dns_key,
            reqwest::Method::DELETE,
            &format!("/v1/boxes/{name}/door"),
            &serde_json::json!({}),
        )
        .await
        .map(|_| ())
    }

    /// The outside check of the door.
    #[allow(dead_code, reason = "the guests' door comes next")]
    pub async fn check_door(&self, dns_key: &SecretKey, name: &str) -> Result<DoorCheck> {
        self.signed(
            dns_key,
            reqwest::Method::POST,
            &format!("/v1/boxes/{name}/check"),
            &serde_json::json!({}),
        )
        .await?
        .context("the server sent no answer")
    }

    /// One-hour credentials for a guest's relay.
    #[allow(dead_code, reason = "guests' WebRTC comes next")]
    pub async fn turn(&self, box_key: &SecretKey, name: &str) -> Result<TurnCredentials> {
        self.signed(
            box_key,
            reqwest::Method::POST,
            &format!("/v1/boxes/{name}/turn"),
            &serde_json::json!({}),
        )
        .await?
        .context("the server sent no answer")
    }

    /// The box's use of the server's relays this month.
    pub async fn relay_use(&self, box_key: &SecretKey, name: &str) -> Result<RelayUse> {
        self.signed(
            box_key,
            reqwest::Method::POST,
            &format!("/v1/boxes/{name}/relay-use"),
            &serde_json::json!({}),
        )
        .await?
        .context("the server sent no answer")
    }

    /// Gives the name up.
    pub async fn release(&self, box_key: &SecretKey, name: &str) -> Result<()> {
        self.signed::<_, serde_json::Value>(
            box_key,
            reqwest::Method::POST,
            &format!("/v1/boxes/{name}/release"),
            &serde_json::json!({}),
        )
        .await
        .map(|_| ())
    }
}

/// The server's own words, if the error came from it.
pub fn said(err: &anyhow::Error) -> Option<&ServerSaid> {
    err.downcast_ref::<ServerSaid>()
}
