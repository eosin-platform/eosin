use anyhow::{Context, Error, Result, anyhow, bail};
use axum::{
    Json, Router,
    extract::{Path, State},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use eosin_common::{access_log, response};
use http::StatusCode;
use owo_colors::OwoColorize;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app::App;
use eosin_iam_client::{
    JwtLike, LoginRequest, RefreshRequest, RegisterRequest, RegisterResponse, User,
    UserCredentials, UserHasRoleResponse, UserRole,
};

pub async fn run_server(cancel: CancellationToken, port: u16, app_state: App) -> Result<()> {
    let health_router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }));
    let app = Router::new()
        .route("/user/register", post(register))
        .route("/user/login", post(login))
        .route("/user/refresh", post(refresh))
        .route("/user/signout", post(sign_out))
        .route(
            "/iam/user/roles/{username_or_id}/{role}",
            get(get_user_role),
        )
        .route("/iam/user/info/{username_or_id}", get(get_user_info))
        .route("/iam/user/lookup/{username}", get(lookup_user))
        .with_state(app_state)
        .layer(middleware::from_fn(access_log::internal));
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .map_err(|e| {
            eprintln!(
                "{}",
                format!("❌ Failed to bind server to {}: {}", addr, e).red()
            );
            e
        })
        .context("Failed to bind server")?;
    println!(
        "{}{}",
        "🚀 Starting internal iam server • port=".green(),
        format!("{}", port).green().dimmed()
    );
    axum::serve(listener, app.merge(health_router))
        .with_graceful_shutdown(async move {
            cancel.cancelled().await;
        })
        .await
        .context("Failed to start server")?;
    println!("{}", "🛑 Internal server stopped gracefully.".red());
    Ok(())
}

fn kc_admin_base(endpoint: &str, realm: &str) -> Result<String> {
    // If endpoint already contains /realms/{realm}, strip from there to get the base host,
    // then build /admin/realms/{realm}.
    let trimmed = endpoint.trim_end_matches('/');
    if let Some(i) = trimmed.find("/realms/") {
        let base_host = &trimmed[..i]; // everything before /realms/
        Ok(format!("{}/admin/realms/{}", base_host, realm))
    } else {
        Ok(format!("{}/admin/realms/{}", trimmed, realm))
    }
}

fn kc_token_url(endpoint: &str, realm: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    if base.contains("/realms/") {
        format!("{}/protocol/openid-connect/token", base)
    } else {
        format!("{}/realms/{}/protocol/openid-connect/token", base, realm)
    }
}

async fn kc_service_token(
    client: &reqwest::Client,
    endpoint: &str,
    realm: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let url = kc_token_url(endpoint, realm);
    #[allow(dead_code)]
    #[derive(serde::Deserialize)]
    struct TokenResp {
        #[serde(default)]
        access_token: String,
        #[serde(default)]
        token_type: Option<String>,
        #[serde(default)]
        expires_in: Option<u64>,
    }

    let res = client
        .post(url)
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ])
        .send()
        .await
        .context("Keycloak token request failed")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak token request failed: {} {}", status, body);
    }

    let token: TokenResp = res
        .json()
        .await
        .context("Failed to parse Keycloak token response")?;
    if token.access_token.is_empty() {
        bail!("Keycloak token response missing access_token");
    }
    Ok(token.access_token)
}

pub async fn user_has_role(state: &App, user_id: Uuid, role: UserRole) -> Result<bool> {
    let http = reqwest::Client::new();

    // 1) Service token (the client used here must have admin perms like view-users/view-clients/view-realm)
    let token = kc_service_token(
        &http,
        &state.kc.endpoint,
        &state.kc.realm,
        &state.kc.client_id,
        &state.kc.client_secret,
    )
    .await
    .context("Failed to obtain Keycloak service token")?;

    // 2) Admin base
    let admin_base = kc_admin_base(&state.kc.endpoint, &state.kc.realm)
        .context("Failed to obtain Keycloak admin base")?;

    // 3) Resolve the client UUID (Keycloak internal ID) for the configured clientId
    #[derive(Deserialize)]
    struct ClientRepr {
        #[serde(default)]
        id: String, // internal UUID
        #[serde(default, rename = "clientId")]
        client_id: String, // human-visible clientId
    }

    let clients_url = Url::parse_with_params(
        &format!("{}/clients", admin_base),
        &[("clientId", state.kc.client_id.as_str())],
    )
    .context("Failed to build clients URL")?;

    let clients_res = http
        .get(clients_url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak clients query failed")?;

    if !clients_res.status().is_success() {
        let status = clients_res.status();
        let body = clients_res.text().await.unwrap_or_default();
        bail!("Keycloak clients query failed: {} {}", status, body);
    }

    let clients: Vec<ClientRepr> = clients_res
        .json()
        .await
        .context("Failed to parse clients response")?;

    let client = clients
        .into_iter()
        .find(|c| c.client_id == state.kc.client_id)
        .ok_or_else(|| {
            anyhow::anyhow!("Client with clientId '{}' not found", state.kc.client_id)
        })?;

    if client.id.is_empty() {
        bail!("Resolved client has empty internal id");
    }

    // 4) Fetch the user's **effective** client role mappings for that client
    //    (use /composite to include composites)
    let roles_url = format!(
        "{}/users/{}/role-mappings/clients/{}/composite",
        admin_base, user_id, client.id
    );

    let roles_res = http
        .get(&roles_url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak user role-mappings request failed")?;

    // 404 means no mappings at all for that client → user clearly doesn't have the role
    if roles_res.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(false);
    }

    if !roles_res.status().is_success() {
        let status = roles_res.status();
        let body = roles_res.text().await.unwrap_or_default();
        bail!("Keycloak user role-mappings failed: {} {}", status, body);
    }

    // Roles come back as an array of role representations with at least a "name"
    let roles_json: Value = roles_res
        .json()
        .await
        .context("Failed to parse user role-mappings response")?;

    let want = role.as_str();

    // Expect array; if not, treat as no roles
    let has = roles_json
        .as_array()
        .map(|arr| {
            arr.iter().any(|r| {
                r.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n == want)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    Ok(has)
}

pub async fn get_user_by_access_token(state: &App, access_token: &str) -> Result<Option<User>> {
    // --- 1) Try to decode the JWT locally (no verification — you've already done that elsewhere)
    #[derive(Deserialize)]
    struct Claims {
        #[serde(default)]
        sub: String,
        #[serde(default)]
        preferred_username: String,
    }

    fn decode_claims(token: &str) -> Result<Claims> {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let mut parts = token.split('.');
        let _header = parts
            .next()
            .ok_or_else(|| anyhow!("Malformed JWT: missing header"))?;
        let payload = parts
            .next()
            .ok_or_else(|| anyhow!("Malformed JWT: missing payload"))?;

        let bytes = URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|e| anyhow!("Failed to base64url-decode JWT payload: {}", e))?;
        let claims: Claims = serde_json::from_slice(&bytes)
            .map_err(|e| anyhow!("Failed to parse JWT claims JSON: {}", e))?;
        Ok(claims)
    }

    // Helper to resolve a user once we have either sub or preferred_username
    async fn resolve_user(
        state: &App,
        sub: &str,
        preferred_username: &str,
    ) -> Result<Option<User>> {
        // Prefer sub as UUID if available
        if let Ok(id) = Uuid::parse_str(sub) {
            match get_user_by_id(state, id).await {
                Ok(u) => return Ok(Some(u)),
                Err(e) => {
                    // If ID lookup failed, try username fallback next (if we have one)
                    if preferred_username.is_empty() {
                        // Nothing else to try
                        return Err(e);
                    }
                }
            }
        }

        if !preferred_username.is_empty() {
            return get_user_by_username(state, preferred_username).await;
        }

        Ok(None)
    }

    // First attempt: local decode
    if let Ok(claims) = decode_claims(access_token)
        && let Ok(user) = resolve_user(state, &claims.sub, &claims.preferred_username).await
    {
        return Ok(user);
        // If that failed due to admin lookup issues, fall through to /userinfo as a secondary source.
    }

    // --- 2) Fallback: call the realm's /userinfo endpoint with the access token
    fn kc_userinfo_url(endpoint: &str, realm: &str) -> String {
        let base = endpoint.trim_end_matches('/');
        if base.contains("/realms/") {
            format!("{}/protocol/openid-connect/userinfo", base)
        } else {
            format!("{}/realms/{}/protocol/openid-connect/userinfo", base, realm)
        }
    }

    let url = kc_userinfo_url(&state.kc.endpoint, &state.kc.realm);
    let http = reqwest::Client::new();
    let res = http.get(url).bearer_auth(access_token).send().await;

    let res = match res {
        Ok(r) => r,
        Err(e) => {
            // Network error talking to userinfo — we can’t improve from here
            return Err(anyhow!("UserInfo request failed: {}", e));
        }
    };

    if res.status() == StatusCode::UNAUTHORIZED {
        // Access token invalid/expired for userinfo
        return Ok(None);
    }

    if !res.status().is_success() {
        let code = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("UserInfo unexpected error: {} {}", code, body);
    }

    let claims: Claims = res
        .json()
        .await
        .context("Failed to parse /userinfo response")?;

    resolve_user(state, &claims.sub, &claims.preferred_username).await
}

pub async fn get_user_by_email(state: &App, email: &str) -> Result<Option<User>> {
    let client = reqwest::Client::new();

    // 1) Get service token (client credentials)
    let token = kc_service_token(
        &client,
        &state.kc.endpoint,
        &state.kc.realm,
        &state.kc.client_id,
        &state.kc.client_secret,
    )
    .await?;

    // 2) Build Admin API URL
    let admin_base = kc_admin_base(&state.kc.endpoint, &state.kc.realm)?;
    let url = Url::parse_with_params(
        &format!("{}/users", admin_base),
        &[("email", email), ("exact", "true")],
    )
    .context("Failed to build Keycloak users search URL")?;

    // 3) Query Keycloak
    let res = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak users search request failed")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak users search failed: {} {}", status, body);
    }

    // 4) Expect an array; pick the first exact match (Keycloak may still return multiple)
    let users_json: Value = res
        .json()
        .await
        .context("Failed to parse Keycloak users search response")?;
    let arr = users_json
        .as_array()
        .ok_or_else(|| anyhow!("Keycloak users search did not return an array"))?;

    if arr.is_empty() {
        return Ok(None);
    }

    let email_lower = email.to_ascii_lowercase();
    let matched = arr.iter().find(|u| {
        u.get("email")
            .and_then(|e| e.as_str())
            .map(|e| e.to_ascii_lowercase() == email_lower)
            .unwrap_or(false)
    });

    let Some(user_val) = matched else {
        return Ok(None);
    };

    let user: User = serde_json::from_value(user_val.clone())
        .context("Failed to deserialize user into your User type; check field mappings/renames")?;
    Ok(Some(user))
}

pub async fn get_user_by_username(state: &App, username: &str) -> Result<Option<User>> {
    let client = reqwest::Client::new();

    // 1) Get service token (client credentials)
    let token = kc_service_token(
        &client,
        &state.kc.endpoint,
        &state.kc.realm,
        &state.kc.client_id,
        &state.kc.client_secret,
    )
    .await?;

    // 2) Build Admin API URL
    let admin_base = kc_admin_base(&state.kc.endpoint, &state.kc.realm)?;
    let url = Url::parse_with_params(
        &format!("{}/users", admin_base),
        &[("username", username), ("exact", "true")],
    )
    .context("Failed to build Keycloak users search URL")?;

    // 3) Query Keycloak
    let res = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak users search request failed")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak users search failed: {} {}", status, body);
    }

    // 4) Expect an array; pick the first exact match
    let users_json: Value = res
        .json()
        .await
        .context("Failed to parse Keycloak users search response")?;
    let arr = users_json
        .as_array()
        .ok_or_else(|| anyhow!("Keycloak users search did not return an array"))?;

    if arr.is_empty() {
        return Ok(None);
    }

    let first = arr.first().unwrap();
    let user: User = serde_json::from_value(first.clone())
        .context("Failed to deserialize user into your User type; check field mappings/renames")?;
    Ok(Some(user))
}

pub async fn get_user_by_id(state: &App, user_id: Uuid) -> Result<User> {
    let client = reqwest::Client::new();

    // 1) Get service token
    let token = kc_service_token(
        &client,
        &state.kc.endpoint,
        &state.kc.realm,
        &state.kc.client_id,
        &state.kc.client_secret,
    )
    .await?;

    // 2) Build Admin API URL: /users/{id}
    let admin_base = kc_admin_base(&state.kc.endpoint, &state.kc.realm)?;
    let url = format!("{}/users/{}", admin_base, user_id);

    // 3) Query Keycloak
    let res = client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak get user by id request failed")?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("No user found for id {}", user_id);
    }
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak get user by id failed: {} {}", status, body);
    }

    // 4) Deserialize into your API's User type
    let user_json: Value = res
        .json()
        .await
        .context("Failed to parse Keycloak user response")?;
    let user: User = serde_json::from_value(user_json)
        .context("Failed to deserialize user into your User type; check field mappings/renames")?;

    Ok(user)
}

pub async fn lookup_user(
    State(state): State<App>,
    Path(username): Path<String>,
) -> impl IntoResponse {
    match get_user_by_username(&state, &username).await {
        Ok(Some(user)) => (StatusCode::OK, user.id.as_bytes().to_vec()).into_response(),
        Ok(None) => response::not_found(anyhow!("User not found")),
        Err(e) => response::error(e.context("Failed to lookup user")),
    }
}

pub async fn get_user_info(
    State(state): State<App>,
    Path(username_or_id): Path<String>,
) -> impl IntoResponse {
    match Uuid::parse_str(&username_or_id) {
        Ok(id) => match get_user_by_id(&state, id).await {
            Ok(user) => (StatusCode::OK, Json(user)).into_response(),
            Err(e) => response::error(e.context("Failed to get user by ID")),
        },
        _ => match get_user_by_username(&state, &username_or_id).await {
            Ok(user) => (StatusCode::OK, Json(user)).into_response(),
            Err(e) => response::error(e.context("Failed to get user by username")),
        },
    }
}

pub async fn get_user_role(
    State(state): State<App>,
    Path((username_or_id, role)): Path<(String, UserRole)>,
) -> impl IntoResponse {
    let user_id = match Uuid::parse_str(&username_or_id) {
        Ok(id) => id,
        _ => match get_user_by_username(&state, &username_or_id).await {
            Ok(user) => match user {
                Some(u) => u.id,
                None => {
                    return response::not_found(anyhow::anyhow!(
                        "User '{}' not found",
                        username_or_id
                    ));
                }
            },
            Err(e) => {
                return response::error(e.context("Failed to get user by username"));
            }
        },
    };
    match user_has_role(&state, user_id, role).await {
        Ok(has_role) => (StatusCode::OK, Json(UserHasRoleResponse { has_role })).into_response(),
        Err(e) => response::error(e.context("Failed to check user role")),
    }
}

pub async fn refresh(
    State(state): State<App>,
    Json(req): Json<RefreshRequest>,
) -> impl IntoResponse {
    // Build the token endpoint (supports KC_ENDPOINT with or without /realms/{realm})
    let url = kc_token_url(&state.kc.endpoint, &state.kc.realm);

    let http = reqwest::Client::new();
    let res = http
        .post(url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", state.kc.client_id.as_str()),
            ("client_secret", state.kc.client_secret.as_str()),
            ("refresh_token", req.refresh_token.as_str()),
            // scope usually not required on refresh, but harmless if included:
            ("scope", "openid"),
        ])
        .send()
        .await;

    let res = match res {
        Ok(v) => v,
        Err(e) => {
            return response::bad_gateway(
                Error::from(e).context("Keycloak refresh request failed"),
            );
        }
    };

    // Keycloak generally returns 200 on success, 400 on invalid_grant, others for infra issues
    if !res.status().is_success() {
        let code = res.status();
        let text = res.text().await.unwrap_or_default();

        // Try to detect invalid/expired refresh token and map to 401
        if code == StatusCode::BAD_REQUEST
            && let Ok(val) = serde_json::from_str::<serde_json::Value>(&text)
        {
            if val
                .get("error")
                .and_then(|e| e.as_str())
                .map(|e| e == "invalid_grant")
                .unwrap_or(false)
            {
                // Optionally log the description if present
                if let Some(desc) = val.get("error_description").and_then(|d| d.as_str()) {
                    eprintln!("Keycloak refresh invalid_grant: {}", desc);
                }
                return response::unauthorized(anyhow::anyhow!("Invalid or expired refresh token"));
            }
            let reason = match val.get("error_description") {
                Some(d) => d.as_str().unwrap_or("unknown reason"),
                None => "unknown reason",
            };
            return response::bad_gateway(
                anyhow::anyhow!("{}", reason).context("Keycloak refresh failed"),
            );
        }

        // Fallback: return the raw body if it wasn't JSON
        eprintln!("Keycloak refresh unexpected error: {} - {}", code, text);
        return (code, text).into_response();
    }

    // Success: parse into your JwtLike and return
    let jwt_like: JwtLike = match res.json().await {
        Ok(v) => v,
        Err(e) => {
            return response::bad_gateway(
                Error::from(e).context("Failed to parse Keycloak refresh response"),
            );
        }
    };

    match get_user_by_access_token(&state, &jwt_like.access_token).await {
        Ok(Some(user)) => {
            println!(
                "{}{}{}{}",
                "♻️ User refreshed token successfully • username=".cyan(),
                user.username.cyan().dimmed(),
                " • id=".cyan(),
                user.id.cyan().dimmed()
            );
            (
                axum::http::StatusCode::OK,
                Json(UserCredentials {
                    id: user.id,
                    email: user.email.unwrap_or_default(),
                    first_name: user.first_name.unwrap_or_default(),
                    last_name: user.last_name.unwrap_or_default(),
                    username: user.username,
                    jwt: jwt_like,
                }),
            )
                .into_response()
        }
        Ok(None) => response::not_found(anyhow!("User not found after token refresh")),
        Err(e) => response::error(e.context("Failed to get user by access token")),
    }
}

pub async fn sign_out(
    State(_state): State<App>,
    Json(_req): Json<LoginRequest>,
) -> impl IntoResponse {
    StatusCode::OK.into_response()
}

fn is_valid_username(username: &str) -> bool {
    // Example validation: non-empty, alphanumeric + underscores, 3-30 chars
    let len = username.len();
    if !(3..=30).contains(&len) {
        return false;
    }
    username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn contains_number(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

fn contains_special_char(s: &str) -> bool {
    s.chars().any(|c| !c.is_alphanumeric())
}

fn is_valid_password(password: &str) -> bool {
    password.len() >= 8 && contains_number(password) && contains_special_char(password)
}

async fn reset_user_password(
    user_id: Uuid,
    new_password: &str,
    admin_base: &str,
    client: reqwest::Client,
    token: String,
) -> Result<()> {
    let url = format!("{}/users/{}/reset-password", admin_base, user_id);
    let body = serde_json::json!({
        "type": "password",
        "value": new_password,
        "temporary": false,
    });
    let res = client
        .put(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
        .context("Keycloak reset password request failed")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak reset password failed: {} {}", status, body);
    }
    Ok(())
}

#[derive(Serialize)]
struct CreateUser {
    username: String,

    #[serde(default, rename = "firstName")]
    first_name: Option<String>,

    #[serde(default, rename = "lastName")]
    last_name: Option<String>,

    email: String,

    enabled: bool,

    #[serde(default, rename = "emailVerified")]
    email_verified: bool,
}

async fn create_user(state: &App, req: RegisterRequest) -> Result<RegisterResponse> {
    let client = reqwest::Client::new();
    let token = kc_service_token(
        &client,
        &state.kc.endpoint,
        &state.kc.realm,
        &state.kc.client_id,
        &state.kc.client_secret,
    )
    .await
    .context("Failed to get service token")?;
    let admin_base = kc_admin_base(&state.kc.endpoint, &state.kc.realm)
        .context("Failed to get Keycloak admin base URL")?;
    let url = format!("{}/users", admin_base);
    let res: reqwest::Response = client
        .post(url)
        .json(&CreateUser {
            username: req.username.clone(),
            first_name: req.first_name,
            last_name: req.last_name,
            email: req.email,
            enabled: true,
            email_verified: true,
        })
        .bearer_auth(&token)
        .send()
        .await
        .context("Keycloak users search request failed")?;
    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("Keycloak user registration failed: {} {}", status, body);
    }
    let user = match get_user_by_username(state, &req.username).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            bail!("User created but cannot be found by username after creation");
        }
        Err(e) => {
            bail!("Failed to get user by username after creation: {}", e);
        }
    };
    reset_user_password(user.id, &req.password, &admin_base, client.clone(), token)
        .await
        .context("Failed to set user password")?;

    // Build the token endpoint. Support KC_ENDPOINT with or without trailing "/realms/{realm}"
    let base = state.kc.endpoint.trim_end_matches('/');
    let url = if base.contains("/realms/") {
        format!("{}/protocol/openid-connect/token", base)
    } else {
        format!(
            "{}/realms/{}/protocol/openid-connect/token",
            base, state.kc.realm
        )
    };
    let res = client
        .post(url)
        .form(&[
            ("grant_type", "password"),
            ("client_id", state.kc.client_id.as_str()),
            ("client_secret", state.kc.client_secret.as_str()),
            ("username", req.username.as_str()),
            ("password", req.password.as_str()),
            ("scope", "openid offline_access"),
        ])
        .send()
        .await
        .context("Keycloak login request failed after registration")?;
    if res.status() == axum::http::StatusCode::UNAUTHORIZED {
        bail!(
            "Invalid username or password for user '{}' after registration",
            req.username
        );
    }
    if !res.status().is_success() {
        let code = res.status();
        let text = res.text().await.unwrap_or_default();
        let error = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
        eprintln!("keycloak login error: {} - {}", code, error);
        bail!("Keycloak login failed with status {}: {}", code, text);
    }
    let jwt_like: JwtLike = res
        .json()
        .await
        .context("Failed to parse Keycloak login response after registration")?;
    Ok(RegisterResponse {
        id: user.id,
        jwt: jwt_like,
    })
}

pub async fn register(
    State(state): State<App>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    if !is_valid_username(&req.username) {
        return response::bad_request(anyhow::anyhow!(
            "Invalid username: must be 3-30 characters, alphanumeric/underscore/dash/dot"
        ));
    }
    if !is_valid_password(&req.password) {
        return response::bad_request(anyhow::anyhow!(
            "Invalid password: must be at least 8 characters, include a number and a special character"
        ));
    }
    match create_user(&state, req.clone()).await {
        Ok(resp) => {
            println!(
                "{}{}{}{}",
                "🆕 User registered successfully • username=".cyan(),
                req.username.cyan().dimmed(),
                " • id=".cyan(),
                resp.id.cyan().dimmed()
            );
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => response::error(e.context("Failed to create user")),
    }
}

pub async fn login(State(state): State<App>, Json(req): Json<LoginRequest>) -> impl IntoResponse {
    // Is the username actually an email?
    let (user, username) = if req.username.contains('@') {
        // Resolve the username based on the email
        match get_user_by_email(&state, &req.username).await {
            Ok(Some(user)) => {
                println!(
                    "{}{}{}{}",
                    "🔓 User logging in by email • email=".cyan(),
                    req.username.cyan().dimmed(),
                    " • resolved username=".cyan(),
                    user.username.cyan().dimmed()
                );
                let username = user.username.clone();
                (Some(user), username)
            }
            Ok(None) => return response::invalid_credentials(),
            Err(e) => return response::error(e.context("Failed to get user by email")),
        }
    } else {
        (None, req.username)
    };

    // Build the token endpoint. Support KC_ENDPOINT with or without trailing "/realms/{realm}"
    let base = state.kc.endpoint.trim_end_matches('/');
    let url = if base.contains("/realms/") {
        format!("{}/protocol/openid-connect/token", base)
    } else {
        format!(
            "{}/realms/{}/protocol/openid-connect/token",
            base, state.kc.realm
        )
    };
    let http = reqwest::Client::new();
    let res = http
        .post(url)
        .form(&[
            ("grant_type", "password"),
            ("client_id", state.kc.client_id.as_str()),
            ("client_secret", state.kc.client_secret.as_str()),
            ("username", username.as_str()),
            ("password", req.password.as_str()),
            ("scope", "openid offline_access"),
        ])
        .send()
        .await;
    let res = match res {
        Ok(v) => v,
        Err(e) => {
            return response::bad_gateway(Error::from(e).context("Keycloak login request failed"));
        }
    };
    if res.status() == axum::http::StatusCode::UNAUTHORIZED {
        return response::invalid_credentials();
    }
    if !res.status().is_success() {
        let code = res.status();
        let text = res.text().await.unwrap_or_default();
        let error = serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default();
        eprintln!("keycloak login error: {} - {}", code, error);
        return (code, Json(error)).into_response();
    }
    let jwt_like: JwtLike = match res.json().await {
        Ok(v) => v,
        Err(e) => {
            return response::bad_gateway(
                Error::from(e).context("Failed to parse Keycloak login response"),
            );
        }
    };
    creds_response(state, &username, user, jwt_like).await
}

async fn creds_response(
    state: App,
    username: &str,
    user: Option<User>,
    jwt_like: JwtLike,
) -> Response {
    let user = match user {
        Some(u) => u,
        None => match get_user_by_username(&state, username).await {
            Ok(Some(u)) => u,
            Ok(None) => {
                return response::not_found(anyhow::anyhow!(
                    "User '{}' not found after login",
                    username
                ));
            }
            Err(e) => {
                return response::error(e.context("Failed to get user by username"));
            }
        },
    };
    println!(
        "{}{}{}{}",
        "🔓 User logged in successfully • username=".cyan(),
        user.username.cyan().dimmed(),
        " • id=".cyan(),
        user.id.cyan().dimmed()
    );
    (
        axum::http::StatusCode::OK,
        Json(UserCredentials {
            id: user.id,
            email: user.email.unwrap_or_default(),
            first_name: user.first_name.unwrap_or_default(),
            last_name: user.last_name.unwrap_or_default(),
            username: user.username,
            jwt: jwt_like,
        }),
    )
        .into_response()
}
