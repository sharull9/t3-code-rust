//! Pairing and session credentials.
//!
//! Flow (see `docs/internals/environment-auth.md` in t3code):
//! 1. The server issues a one-time pairing link: `http://host:port/pair#token=...`
//!    (`npx t3 serve` prints one, or `npx t3 auth pairing create`).
//! 2. `POST /oauth/token` exchanges it for a bearer access token.
//! 3. Before each socket connect, `POST /api/auth/websocket-ticket` mints a
//!    short-lived ticket so the long-lived token stays out of socket URLs.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::Error;

const GRANT_TYPE_TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const TOKEN_TYPE_BOOTSTRAP: &str = "urn:t3:params:oauth:token-type:environment-bootstrap";
const TOKEN_TYPE_ACCESS: &str = "urn:ietf:params:oauth:token-type:access_token";
// Matches packages/contracts/src/environment.ts in T3 Code.
const ORCHESTRATION_PROTOCOL_VERSION: &str = "2";

/// A parsed pairing link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLink {
    pub base_url: Url,
    pub token: String,
}

impl PairingLink {
    /// Server used when only a token is given, matching `npx t3`'s default port.
    pub const DEFAULT_SERVER: &str = "http://localhost:3773/";

    /// Accepts any of:
    /// - a pairing link, `http://host:port/pair#token=...` (or a `?token=` query)
    /// - a server and a token separated by whitespace, `http://host:port TOKEN`
    /// - a bare token, as printed by `npx t3 auth pairing create`, for
    ///   [`Self::DEFAULT_SERVER`]
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut parts = input.split_whitespace();
        match (parts.next(), parts.next(), parts.next()) {
            (Some(link), None, None) if link.contains("://") => Self::parse_link(link),
            (Some(token), None, None) => Self::with_server(Self::DEFAULT_SERVER, token),
            (Some(server), Some(token), None) => Self::with_server(server, token),
            _ => Err(Error::InvalidPairingLink(
                "paste a pairing link, a token, or `<server url> <token>`".into(),
            )),
        }
    }

    fn with_server(server: &str, token: &str) -> Result<Self, Error> {
        let url = Url::parse(server)?;
        Self::check_scheme(&url)?;
        Ok(Self { base_url: base_of(&url), token: token.to_owned() })
    }

    fn parse_link(link: &str) -> Result<Self, Error> {
        let url = Url::parse(link)?;
        Self::check_scheme(&url)?;
        let from_fragment = url.fragment().and_then(|fragment| {
            url::form_urlencoded::parse(fragment.as_bytes())
                .find(|(key, _)| key == "token")
                .map(|(_, value)| value.into_owned())
        });
        let from_query = || {
            url.query_pairs().find(|(key, _)| key == "token").map(|(_, value)| value.into_owned())
        };
        let token = from_fragment
            .or_else(from_query)
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| Error::InvalidPairingLink("missing `token`".into()))?;
        Ok(Self { base_url: base_of(&url), token })
    }

    fn check_scheme(url: &Url) -> Result<(), Error> {
        match url.scheme() {
            "http" | "https" => Ok(()),
            other => Err(Error::InvalidPairingLink(format!("unsupported scheme `{other}`"))),
        }
    }
}

fn base_of(url: &Url) -> Url {
    let mut base_url = url.clone();
    base_url.set_path("/");
    base_url.set_query(None);
    base_url.set_fragment(None);
    base_url
}

/// Durable credentials for one environment. Safe to persist; treat as a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub base_url: Url,
    pub access_token: String,
    pub token_type: String,
    pub scope: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl Credentials {
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() >= self.expires_at
    }

    fn authorization(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }
}

#[derive(Deserialize)]
struct AccessTokenResult {
    access_token: String,
    token_type: String,
    expires_in: f64,
    scope: String,
}

#[derive(Deserialize)]
struct WebSocketTicketResult {
    ticket: String,
}

/// Exchange a one-time pairing link for durable credentials.
pub async fn pair(
    http: &reqwest::Client,
    link: &PairingLink,
    client_label: &str,
) -> Result<Credentials, Error> {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    let response = http
        .post(link.base_url.join("/oauth/token")?)
        .form(&[
            ("grant_type", GRANT_TYPE_TOKEN_EXCHANGE),
            ("subject_token", link.token.as_str()),
            ("subject_token_type", TOKEN_TYPE_BOOTSTRAP),
            ("requested_token_type", TOKEN_TYPE_ACCESS),
            ("client_label", client_label),
            ("client_device_type", "desktop"),
            ("client_os", os),
        ])
        .send()
        .await?;
    let result: AccessTokenResult = read_json(response).await?;
    // DPoP-bound tokens need proof keys; we never request them, so this is
    // only a guard against a server that insists.
    if !result.token_type.eq_ignore_ascii_case("bearer") {
        return Err(Error::Http {
            status: 0,
            body: format!("unsupported token type `{}`", result.token_type),
        });
    }
    Ok(Credentials {
        base_url: link.base_url.clone(),
        access_token: result.access_token,
        token_type: "Bearer".into(),
        scope: result.scope,
        expires_at: chrono::Utc::now().timestamp() + result.expires_in as i64,
    })
}

/// Mint a short-lived WebSocket ticket and return the full `/ws` URL.
pub async fn websocket_url(
    http: &reqwest::Client,
    credentials: &Credentials,
) -> Result<Url, Error> {
    let response = http
        .post(credentials.base_url.join("/api/auth/websocket-ticket")?)
        .header(reqwest::header::AUTHORIZATION, credentials.authorization())
        .send()
        .await?;
    let WebSocketTicketResult { ticket } = read_json(response).await?;

    websocket_url_with_ticket(&credentials.base_url, &ticket)
}

fn websocket_url_with_ticket(base_url: &Url, ticket: &str) -> Result<Url, Error> {
    let mut url = base_url.join("/ws")?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme)
        .map_err(|()| Error::InvalidPairingLink("cannot build websocket url".into()))?;
    url.query_pairs_mut()
        .append_pair("wsTicket", ticket)
        .append_pair("orchestrationProtocol", ORCHESTRATION_PROTOCOL_VERSION);
    Ok(url)
}

async fn read_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, Error> {
    let status = response.status();
    if status.is_success() {
        return Ok(response.json().await?);
    }
    let body = response.text().await.unwrap_or_default();
    Err(match status.as_u16() {
        401 | 403 => Error::Unauthorized { status: status.as_u16(), body },
        code => Error::Http { status: code, body },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_handshake_declares_v2_and_preserves_ticket() {
        for (base, scheme) in
            [("http://localhost:3773/", "ws"), ("https://box.tail.ts.net:4000/", "wss")]
        {
            let base = Url::parse(base).unwrap();
            let url = websocket_url_with_ticket(&base, "ticket+with/special?characters&").unwrap();
            assert_eq!(url.scheme(), scheme);
            assert_eq!(url.host_str(), base.host_str());
            assert_eq!(url.port(), base.port());
            assert_eq!(url.path(), "/ws");
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query.get("wsTicket").unwrap(), "ticket+with/special?characters&");
            assert_eq!(query.get("orchestrationProtocol").unwrap(), "2");
        }
    }

    #[test]
    fn parses_fragment_pairing_link() {
        let link = PairingLink::parse("http://localhost:3773/pair#token=abc123").unwrap();
        assert_eq!(link.base_url.as_str(), "http://localhost:3773/");
        assert_eq!(link.token, "abc123");
    }

    #[test]
    fn parses_query_pairing_link() {
        let link = PairingLink::parse("https://box.tail.ts.net/?token=xyz").unwrap();
        assert_eq!(link.base_url.as_str(), "https://box.tail.ts.net/");
        assert_eq!(link.token, "xyz");
    }

    #[test]
    fn bare_token_uses_default_server() {
        let link = PairingLink::parse("  7NB9KZSDLQLW \n").unwrap();
        assert_eq!(link.base_url.as_str(), PairingLink::DEFAULT_SERVER);
        assert_eq!(link.token, "7NB9KZSDLQLW");
    }

    #[test]
    fn server_and_token() {
        let link = PairingLink::parse("http://127.0.0.1:3899 ABC123").unwrap();
        assert_eq!(link.base_url.as_str(), "http://127.0.0.1:3899/");
        assert_eq!(link.token, "ABC123");
    }

    #[test]
    fn rejects_link_without_token() {
        assert!(PairingLink::parse("http://localhost:3773/pair").is_err());
    }
}
