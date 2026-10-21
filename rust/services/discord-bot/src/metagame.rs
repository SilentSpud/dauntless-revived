//! The metagame on this machine (`backend` in `keys.mjs`, `recoveryRequest` in `recovery.mjs`):
//! loopback HTTP only, redirects refused, so the admin key and the recovery secret never leave it.

use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use serde_json::{Value, json};
use url::Url;

use crate::keys::{Backend, BoxFuture, Failure, Row};

/// One answer: status and body bytes.
pub struct Answer {
    pub status: u16,
    pub body: Bytes,
}

/// How requests go out; tests answer them instead.
pub trait Transport: Send + Sync {
    fn request(
        &self,
        method: &str,
        url: Url,
        headers: Vec<(String, String)>,
        body: Option<String>,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<Answer, Failure>>;
}

/// Plain HTTP with hyper. A redirect is an answer like any other, never followed.
pub struct Http {
    client: Client<HttpConnector, Full<Bytes>>,
}

impl Default for Http {
    fn default() -> Self {
        Self {
            client: Client::builder(TokioExecutor::new()).build_http(),
        }
    }
}

impl Transport for Http {
    fn request(
        &self,
        method: &str,
        url: Url,
        headers: Vec<(String, String)>,
        body: Option<String>,
        timeout: Duration,
    ) -> BoxFuture<'_, Result<Answer, Failure>> {
        let method = method.to_owned();
        Box::pin(async move {
            let mut request = http::Request::builder().method(method.as_str()).uri(url.as_str());
            for (name, value) in &headers {
                request = request.header(name.as_str(), value.as_str());
            }
            let request = request
                .body(Full::new(Bytes::from(body.unwrap_or_default())))
                .map_err(|e| Failure::new(e.to_string()))?;
            let exchange = async {
                let response = self
                    .client
                    .request(request)
                    .await
                    .map_err(|e| Failure::new(e.to_string()))?;
                let status = response.status().as_u16();
                // A redirect is never followed
                if (300..400).contains(&status) {
                    return Err(Failure::new("Redirect refused"));
                }
                let body = response
                    .into_body()
                    .collect()
                    .await
                    .map_err(|e| Failure::new(e.to_string()))?
                    .to_bytes();
                Ok(Answer { status, body })
            };
            tokio::time::timeout(timeout, exchange)
                .await
                .unwrap_or_else(|_| Err(Failure::new("The operation was aborted due to timeout")))
        })
    }
}

/// The metagame's base URL: plain HTTP on 127.0.0.1, nothing else in it.
pub fn loopback_base(base: &str) -> Result<Url, String> {
    let url = Url::parse(base).map_err(|_| "Metagame must use loopback HTTP".to_owned())?;
    let plain = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none();
    if plain {
        Ok(url)
    } else {
        Err("Metagame must use loopback HTTP".into())
    }
}

pub struct MetagameBackend<T: Transport> {
    base: Url,
    key: String,
    transport: T,
}

impl<T: Transport> MetagameBackend<T> {
    pub fn new(base: &str, key: &str, transport: T) -> Result<Self, String> {
        Ok(Self {
            base: loopback_base(base)?,
            key: key.to_owned(),
            transport,
        })
    }

    async fn api(&self, path: &str, body: Option<Value>) -> Result<Option<Value>, Failure> {
        let url = self
            .base
            .join(&format!("/undaunted/api/{path}"))
            .map_err(|e| Failure::new(e.to_string()))?;
        let headers = vec![
            ("x-undaunted-user-api-key".to_owned(), self.key.clone()),
            ("content-type".to_owned(), "application/json".to_owned()),
        ];
        let method = if body.is_some() { "POST" } else { "GET" };
        let answer = self
            .transport
            .request(
                method,
                url,
                headers,
                body.map(|b| b.to_string()),
                Duration::from_secs(15),
            )
            .await?;
        if !(200..300).contains(&answer.status) {
            return Err(Failure {
                message: "Metagame request failed".into(),
                code: None,
                status: Some(answer.status),
            });
        }
        if path == "RegisterInviteCode" {
            return Ok(None);
        }
        serde_json::from_slice(&answer.body)
            .map(Some)
            .map_err(|_| Failure::new("Invalid metagame response"))
    }
}

impl<T: Transport> Backend for MetagameBackend<T> {
    fn link_account(&self, discord_id: &str, user_id: &str) -> BoxFuture<'_, Result<Value, Failure>> {
        let body = json!({ "DiscordId": discord_id, "UserId": user_id });
        Box::pin(async move { Ok(self.api("DiscordLink", Some(body)).await?.unwrap_or(Value::Null)) })
    }

    fn linked_account(&self, discord_id: &str) -> BoxFuture<'_, Result<Option<Value>, Failure>> {
        let path = format!("DiscordLink/{discord_id}");
        Box::pin(async move {
            let result = self.api(&path, None).await?.unwrap_or(Value::Null);
            let account = result["account"].clone();
            Ok((!account.is_null() && account != false).then_some(account))
        })
    }

    fn find(&self, code: &str) -> BoxFuture<'_, Result<Option<Row>, Failure>> {
        let code = code.to_owned();
        Box::pin(async move {
            let result = self.api("InviteCodes", None).await?.unwrap_or(Value::Null);
            let rows = result["InviteCodes"]
                .as_array()
                .ok_or_else(|| Failure::new("Invalid metagame response"))?;
            Ok(rows
                .iter()
                .find(|row| row["inviteCode"] == code.as_str())
                .map(|row| Row {
                    uses_remaining: row["usesRemaining"].clone(),
                    infinite_uses: row["infiniteUses"] == true,
                }))
        })
    }

    fn create(&self, code: &str) -> BoxFuture<'_, Result<(), Failure>> {
        let body = json!({ "NewInviteCode": code, "Uses": 1, "InfiniteUses": false });
        Box::pin(async move { self.api("RegisterInviteCode", Some(body)).await.map(|_| ()) })
    }

    fn identity(&self, key: &str) -> BoxFuture<'_, Result<Option<(String, String)>, Failure>> {
        let key = key.to_owned();
        Box::pin(async move {
            let url = self
                .base
                .join("/undaunted/api/GetUserInfo")
                .map_err(|e| Failure::new(e.to_string()))?;
            let headers = vec![("x-undaunted-user-api-key".to_owned(), key)];
            let answer = self
                .transport
                .request("GET", url, headers, None, Duration::from_secs(5))
                .await?;
            if answer.status == 401 || answer.status == 403 {
                return Ok(None);
            }
            if !(200..300).contains(&answer.status) {
                return Err(Failure::new("Account verification unavailable"));
            }
            let account: Value = serde_json::from_slice(&answer.body)
                .map_err(|_| Failure::new("Unexpected account verification response"))?;
            let user_id = account["UserId"].as_str().unwrap_or("");
            let valid_id = user_id.strip_prefix("UID-").is_some_and(|rest| {
                (1..=100).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
            let Some(username) = account["Username"].as_str().filter(|_| valid_id) else {
                return Err(Failure::new("Unexpected account verification response"));
            };
            Ok(Some((user_id.to_owned(), username.to_owned())))
        })
    }
}

/// `recoveryRequest`: the identity is always the Discord interaction's actor.
pub async fn recovery_request(
    transport: &dyn Transport,
    base: &str,
    secret: Option<&str>,
    action: &str,
    discord_id: &str,
    mut extra: Value,
) -> Result<Value, Failure> {
    let base = Url::parse(base).map_err(|_| Failure::new("Recovery requires loopback"))?;
    if base.scheme() != "http"
        || base.host_str() != Some("127.0.0.1")
        || !base.username().is_empty()
        || base.password().is_some()
    {
        return Err(Failure::new("Recovery requires loopback"));
    }
    let Some(secret) = secret.filter(|s| s.len() >= 32) else {
        return Err(Failure::new("Recovery not configured"));
    };
    if !extra.is_object() {
        extra = json!({});
    }
    extra["discordId"] = json!(discord_id);
    let url = base
        .join(&format!("/internal/key-recovery/{action}"))
        .map_err(|e| Failure::new(e.to_string()))?;
    let headers = vec![
        ("content-type".to_owned(), "application/json".to_owned()),
        ("x-key-recovery-service".to_owned(), secret.to_owned()),
    ];
    let answer = transport
        .request("POST", url, headers, Some(extra.to_string()), Duration::from_secs(15))
        .await?;
    if !(200..300).contains(&answer.status) {
        return Err(Failure::new("Recovery unavailable"));
    }
    serde_json::from_slice(&answer.body).map_err(|_| Failure::new("Recovery unavailable"))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Seen = Arc<Mutex<Vec<(String, String, Vec<(String, String)>, Option<String>)>>>;

    struct Fake {
        seen: Seen,
        status: u16,
        body: String,
    }

    impl Transport for Fake {
        fn request(
            &self,
            method: &str,
            url: Url,
            headers: Vec<(String, String)>,
            body: Option<String>,
            _timeout: Duration,
        ) -> BoxFuture<'_, Result<Answer, Failure>> {
            self.seen
                .lock()
                .unwrap()
                .push((method.to_owned(), url.to_string(), headers, body));
            let answer = Answer {
                status: self.status,
                body: Bytes::from(self.body.clone()),
            };
            Box::pin(async move { Ok(answer) })
        }
    }

    fn fake(status: u16, body: &str) -> (Fake, Seen) {
        let seen: Seen = Arc::default();
        (
            Fake {
                seen: seen.clone(),
                status,
                body: body.to_owned(),
            },
            seen,
        )
    }

    #[tokio::test]
    async fn credentials_never_go_to_remote_urls() {
        for url in [
            "https://example.com",
            "http://example.com",
            "http://127.0.0.1@evil.test",
            "http://127.0.0.1/path",
        ] {
            assert!(MetagameBackend::new(url, "secret", Http::default()).is_err(), "{url}");
        }
        let (transport, seen) = fake(200, "{\"InviteCodes\":[]}");
        let api = MetagameBackend::new("http://127.0.0.1:61000", "secret", transport).unwrap();
        assert_eq!(api.find("DR-test").await.unwrap(), None);
        let seen = seen.lock().unwrap();
        assert!(
            seen[0]
                .2
                .contains(&("x-undaunted-user-api-key".into(), "secret".into()))
        );
    }

    #[tokio::test]
    async fn identity_sends_the_supplied_key_to_loopback_only() {
        let (transport, seen) = fake(200, "{\"UserId\":\"UID-account\",\"Username\":\"Slayer\"}");
        let api = MetagameBackend::new("http://127.0.0.1:61000", "admin-secret", transport).unwrap();
        assert_eq!(
            api.identity("player-key").await.unwrap(),
            Some(("UID-account".into(), "Slayer".into()))
        );
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen[0].1, "http://127.0.0.1:61000/undaunted/api/GetUserInfo");
            assert_eq!(
                seen[0].2,
                [("x-undaunted-user-api-key".to_owned(), "player-key".to_owned())]
            );
        }
        let (transport, _) = fake(401, "");
        let api = MetagameBackend::new("http://127.0.0.1:61000", "admin-secret", transport).unwrap();
        assert_eq!(api.identity("bad-key").await.unwrap(), None);
    }

    #[tokio::test]
    async fn recovery_binds_the_actor_and_its_own_secret() {
        let (transport, seen) = fake(200, "{\"challenge\":\"test\"}");
        let secret = "test-only-".repeat(5);
        let result = recovery_request(
            &transport,
            "http://127.0.0.1:61000",
            Some(&secret),
            "begin",
            "123456789012345678",
            json!({ "discordId": "999999999999999999" }),
        )
        .await
        .unwrap();
        assert_eq!(result["challenge"], "test");
        {
            let seen = seen.lock().unwrap();
            let body: Value = serde_json::from_str(seen[0].3.as_deref().unwrap()).unwrap();
            assert_eq!(body["discordId"], "123456789012345678");
            assert!(!seen[0].2.iter().any(|(name, _)| name == "x-undaunted-user-api-key"));
        }
        assert!(
            recovery_request(
                &transport,
                "http://127.0.0.1:61000",
                Some("short"),
                "begin",
                "1",
                json!({})
            )
            .await
            .is_err()
        );
    }
}
