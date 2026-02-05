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
    /// Full size of the original slide file in bytes
    pub full_size: i64,
    /// Current processing progress in steps of 10,000 tiles
    pub progress_steps: i32,
    /// Total tiles to process
    pub progress_total: i32,
}

/// Request to create a new slide
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSlideRequest {
    pub id: Uuid,
    pub width: i32,
    pub height: i32,
    pub url: String,
    /// Full size of the original slide file in bytes
    pub full_size: i64,
}

/// Request to update slide progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSlideProgressRequest {
    pub progress_steps: i32,
    pub progress_total: i32,
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
    pub async fn create_slide(
        &self,
        id: Uuid,
        width: u32,
        height: u32,
        url: &str,
        full_size: i64,
    ) -> Result<Slide> {
        let request = CreateSlideRequest {
            id,
            width: width as i32,
            height: height as i32,
            url: url.to_string(),
            full_size,
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

    /// Update slide progress
    pub async fn update_progress(
        &self,
        id: Uuid,
        progress_steps: i32,
        progress_total: i32,
    ) -> Result<()> {
        let request = UpdateSlideProgressRequest {
            progress_steps,
            progress_total,
        };

        let response = self
            .client
            .put(format!("{}/slides/{id}/progress", &self.base_url))
            .json(&request)
            .send()
            .await
            .context("failed to send update progress request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("failed to update progress: {status} - {body}");
        }

        Ok(())
    }
}
