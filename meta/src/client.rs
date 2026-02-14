use anyhow::{Context, Result, bail};
use reqwest::Client;
use uuid::Uuid;

use crate::models::{
    CreateSlideRequest, ListSlidesRequest, ListSlidesResponse, Slide, UpdateSlideRequest,
};

/// Client for interacting with the Meta service HTTP API.
#[derive(Clone)]
pub struct MetaClient {
    client: Client,
    base_url: String,
}

impl MetaClient {
    /// Create a new MetaClient with the given base URL.
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Check if the service is healthy.
    pub async fn health(&self) -> Result<()> {
        let url = format!("{}/healthz", self.base_url);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send health request")?;

        if resp.status().is_success() {
            Ok(())
        } else {
            bail!("health check failed with status: {}", resp.status())
        }
    }

    /// Create a new slide.
    pub async fn create_slide(
        &self,
        id: Uuid,
        dataset: Uuid,
        width: i32,
        height: i32,
        url: &str,
        filename: &str,
        full_size: i64,
        metadata: Option<serde_json::Value>,
    ) -> Result<Slide> {
        let api_url = format!("{}/slides", self.base_url);
        let req = CreateSlideRequest {
            id,
            dataset,
            width,
            height,
            url: url.to_string(),
            filename: filename.to_string(),
            full_size,
            metadata,
        };

        let resp = self
            .client
            .post(&api_url)
            .json(&req)
            .send()
            .await
            .context("failed to send create slide request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("create slide failed with status {}: {}", status, body);
        }

        resp.json::<Slide>()
            .await
            .context("failed to parse create slide response")
    }

    /// Get a slide by ID.
    pub async fn get_slide(&self, id: Uuid) -> Result<Option<Slide>> {
        let url = format!("{}/slides/{}", self.base_url, id);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to send get slide request")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("get slide failed with status {}: {}", status, body);
        }

        let slide = resp
            .json::<Slide>()
            .await
            .context("failed to parse get slide response")?;

        Ok(Some(slide))
    }

    /// Update a slide by ID.
    pub async fn update_slide(
        &self,
        id: Uuid,
        dataset: Option<Uuid>,
        width: Option<i32>,
        height: Option<i32>,
        url: Option<String>,
        filename: Option<String>,
        full_size: Option<i64>,
    ) -> Result<Option<Slide>> {
        let api_url = format!("{}/slides/{}", self.base_url, id);
        let req = UpdateSlideRequest {
            dataset,
            width,
            height,
            url,
            filename,
            full_size,
            metadata: None,
        };

        let resp = self
            .client
            .patch(&api_url)
            .json(&req)
            .send()
            .await
            .context("failed to send update slide request")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("update slide failed with status {}: {}", status, body);
        }

        let slide = resp
            .json::<Slide>()
            .await
            .context("failed to parse update slide response")?;

        Ok(Some(slide))
    }

    /// Delete a slide by ID.
    pub async fn delete_slide(&self, id: Uuid) -> Result<bool> {
        let url = format!("{}/slides/{}", self.base_url, id);

        let resp = self
            .client
            .delete(&url)
            .send()
            .await
            .context("failed to send delete slide request")?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }

        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(true);
        }

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("delete slide failed with status {}: {}", status, body);
        }

        Ok(true)
    }

    /// List slides with pagination.
    pub async fn list_slides(&self, offset: i64, limit: i64) -> Result<ListSlidesResponse> {
        let url = format!("{}/slides", self.base_url);
        let req = ListSlidesRequest { offset, limit };

        let resp = self
            .client
            .get(&url)
            .query(&req)
            .send()
            .await
            .context("failed to send list slides request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("list slides failed with status {}: {}", status, body);
        }

        resp.json::<ListSlidesResponse>()
            .await
            .context("failed to parse list slides response")
    }
}
