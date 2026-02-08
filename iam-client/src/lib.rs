use std::{collections::HashMap, fmt::Display, ops::Deref, sync::Arc};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub enum UserRole {
    Admin,
}

impl UserRole {
    pub fn as_str(&self) -> &str {
        match self {
            UserRole::Admin => "admin",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(UserRole::Admin),
            _ => None,
        }
    }
}

impl Serialize for UserRole {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for UserRole {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        UserRole::from_str(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid UserRole string: {}", s)))
    }
}

impl Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserRole::Admin => write!(f, "admin"),
        }
    }
}

pub struct ClientInner {
    pub client: reqwest::Client,
    pub endpoint: String,
    pub jwt: Option<JwtLike>,
}

#[derive(Clone)]
pub struct Client {
    inner: Arc<ClientInner>,
}

impl Deref for Client {
    type Target = ClientInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct User {
    pub id: Uuid,

    pub username: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<HashMap<String, Vec<String>>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_actions: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterResponse {
    pub id: Uuid,
    pub jwt: JwtLike,
}

#[derive(Serialize, Deserialize)]
pub struct UserExistsRequest {
    #[serde(rename = "u", skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct UserExistsResponse {
    pub exists: bool,
}

#[derive(Serialize, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

// Request body for password grant
#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct JwtLike {
    #[serde(default, rename = "access_token")]
    pub access_token: String,
    #[serde(default, rename = "refresh_token")]
    pub refresh_token: Option<String>,
    #[serde(default, rename = "token_type")]
    pub token_type: Option<String>,
    #[serde(default, rename = "expires_in")]
    pub expires_in: Option<u64>,
    #[serde(default, rename = "refresh_expires_in")]
    pub refresh_expires_in: Option<u64>,
    #[serde(default, rename = "id_token")]
    pub id_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub session_state: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
pub struct UserCredentials {
    pub id: Uuid,
    pub username: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub jwt: JwtLike,
}

impl Client {
    pub fn new(endpoint: &str) -> Self {
        Self::new_with_jwt(endpoint, None)
    }

    pub fn new_with_jwt(endpoint: &str, jwt: Option<JwtLike>) -> Self {
        let client = reqwest::Client::new();
        Self {
            inner: Arc::new(ClientInner {
                client,
                endpoint: endpoint.trim_end_matches('/').to_string(),
                jwt,
            }),
        }
    }

    fn with_auth(&self, resp: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(jwt) = &self.jwt {
            resp.bearer_auth(&jwt.access_token)
        } else {
            resp
        }
    }

    pub async fn lookup_user(&self, username: &str) -> Result<Option<Uuid>> {
        let url = format!("{}/iam/user/lookup/{}", self.endpoint, username);
        let resp = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .context("Failed to send get user request")?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            _ => resp
                .error_for_status()
                .context("get user request returned error status")?
                .bytes()
                .await
                .context("Failed to parse response")
                .and_then(|bytes| {
                    Uuid::from_slice(&bytes)
                        .map(Some)
                        .map_err(|e| anyhow!("Failed to parse UUID from response: {}", e))
                }),
        }
    }

    pub async fn delete_user(&self, id: Uuid) -> Result<()> {
        let url = format!("{}/iam/user/{}", self.endpoint, id);
        self.with_auth(self.client.delete(&url))
            .send()
            .await
            .context("Failed to send delete user request")?
            .error_for_status()
            .context("delete user request returned error status")?;
        Ok(())
    }

    pub async fn register(&self, req: &RegisterRequest) -> Result<RegisterResponse> {
        let url = format!("{}/user/register", self.endpoint);
        let resp = self
            .with_auth(self.client.post(&url))
            .json(req)
            .send()
            .await
            .context("Failed to send get user request")?
            .error_for_status()
            .context("Server returned error status")?;
        Ok(resp
            .json()
            .await
            .context("Failed to parse register response")?)
    }

    pub async fn user_has_role(&self, id: Uuid, role: UserRole) -> Result<bool> {
        let url = format!("{}/iam/user/roles/{}/{}", self.endpoint, id, role);
        let resp: UserHasRoleResponse = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .context("Failed to send request")?
            .error_for_status()
            .context("Server returned error status")?
            .json()
            .await
            .context("Failed to parse response")?;
        Ok(resp.has_role)
    }

    pub async fn get_user(&self, id: Uuid) -> Result<Option<User>> {
        self.get_user_by_id_or_username(id.to_string().as_str())
            .await
    }

    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.get_user_by_id_or_username(username).await
    }

    async fn get_user_by_id_or_username(&self, id_or_username: &str) -> Result<Option<User>> {
        let url = format!("{}/iam/user/info/{}", self.endpoint, id_or_username);
        let resp = self
            .with_auth(self.client.get(&url))
            .send()
            .await
            .context("Failed to send get user request")?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            _ => resp
                .error_for_status()
                .context("get user request returned error status")?
                .json()
                .await
                .context("Failed to parse response"),
        }
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<UserCredentials> {
        self.client
            .post(&format!("{}/user/login", self.endpoint))
            .json(&LoginRequest {
                username: username.to_string(),
                password: password.to_string(),
            })
            .send()
            .await
            .context("Failed to send login request")?
            .error_for_status()
            .context("login request returned error status")?
            .json()
            .await
            .context("Failed to parse login response")
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<UserCredentials> {
        self.client
            .post(&format!("{}/user/refresh", self.endpoint))
            .json(&RefreshRequest {
                refresh_token: refresh_token.to_string(),
            })
            .send()
            .await
            .context("Failed to send refresh request")?
            .error_for_status()
            .context("refresh request returned error status")?
            .json()
            .await
            .context("Failed to parse refresh response")
    }
}

#[derive(Serialize, Deserialize)]
pub struct GetUser {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
}

#[derive(Serialize, Deserialize)]
pub struct UserHasRoleResponse {
    pub has_role: bool,
}
