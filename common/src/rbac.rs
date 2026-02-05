use axum::{
    Extension, RequestPartsExt,
    extract::FromRequestParts,
    http::request::Parts,
    response::{IntoResponse, Response},
};
use axum_keycloak_auth::decode::KeycloakToken;
use owo_colors::OwoColorize;
use reqwest::StatusCode;
use uuid::Uuid;

pub struct UserId(pub Uuid);

impl<S> FromRequestParts<S> for UserId
where
    S: Send + Sync,
{
    type Rejection = BadRequest;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Extension(token) = parts
            .extract::<Extension<KeycloakToken<String>>>()
            .await
            .map_err(|e| {
                eprintln!(
                    "{}",
                    format!("❌ Failed to extract Keycloak token: {:?}", e).red()
                );
                BadRequest
            })?;
        let uuid = match Uuid::parse_str(&token.subject) {
            Ok(uuid) => uuid,
            Err(e) => {
                eprintln!(
                    "{}",
                    format!("❌ Failed to parse user ID as UUID: {:?}", e).red()
                );
                return Err(BadRequest);
            }
        };
        Ok(UserId(uuid))
    }
}

pub struct BadRequest;

impl IntoResponse for BadRequest {
    fn into_response(self) -> Response {
        StatusCode::BAD_REQUEST.into_response()
    }
}
