use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Slide metadata returned from the meta service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
}

/// Request to create a new slide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSlideRequest {
    pub width: i32,
    pub height: i32,
    pub url: String,
}

/// Client for the meta HTTP API
#[derive(Clone)]
pub struct MetaClient {
    client: reqwest::Client,
    base_url: String,
}

impl MetaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
        }
    }

    /// Create a new slide in the meta service
    #[allow(clippy::cast_possible_wrap)]
    pub async fn create_slide(&self, width: u32, height: u32, url: &str) -> Result<Slide> {
        let request = CreateSlideRequest {
            width: width as i32,
            height: height as i32,
            url: url.to_string(),
        };

        let response = self
            .client
            .post(format!("{}/slides", &self.base_url))
            .json(&request)
            .send()
            .await
            .context("failed to send create slide request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("failed to create slide: {status} - {body}");
        }

        response
            .json()
            .await
            .context("failed to parse create slide response")
    }

    /// Get a slide by ID
    #[allow(dead_code)]
    pub async fn get_slide(&self, id: Uuid) -> Result<Option<Slide>> {
        let response = self
            .client
            .get(format!("{}/slides/{id}", &self.base_url))
            .send()
            .await
            .context("failed to send get slide request")?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("failed to get slide: {status} - {body}");
        }

        Ok(Some(
            response
                .json()
                .await
                .context("failed to parse get slide response")?,
        ))
    }
}
