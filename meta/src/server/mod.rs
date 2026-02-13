//! Server module for the metadata service.
//!
//! This module provides two separate HTTP servers:
//! - Internal server: For internal service-to-service communication (no auth required)
//! - Public server: For external clients (auth required, injects user_id)

pub mod internal;
pub mod public;

use deadpool_postgres::Pool;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
}
