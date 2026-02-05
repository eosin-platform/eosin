use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub endpoint: Option<String>,
    pub default_workspace_id: Option<Uuid>,
}

pub fn config_path(path: Option<&str>) -> Result<PathBuf> {
    if let Some(path) = path {
        Ok(PathBuf::from(path))
    } else if let Some(path) = std::env::var_os("PSYSLOP_CONFIG") {
        Ok(PathBuf::from(path))
    } else {
        match std::env::home_dir() {
            Some(dir) => Ok(dir.join(".psyslop/config")),
            None => Err(anyhow::anyhow!(
                "Failed to determine home directory for writing config"
            )),
        }
    }
}

pub async fn write_config(path: Option<&str>, config: &Config) -> Result<()> {
    let config_path = config_path(path)?;
    let contents =
        serde_json::to_string_pretty(config).context("Failed to serialize config to JSON")?;
    tokio::fs::create_dir_all(
        config_path
            .parent()
            .context("Failed to get parent directory of config path")?,
    )
    .await
    .context("Failed to create config directory")?;
    tokio::fs::write(&config_path, contents)
        .await
        .context("Failed to write config file")?;
    Ok(())
}

pub async fn default_workspace_id() -> Result<Uuid> {
    load_config()
        .await?
        .and_then(|cfg| cfg.default_workspace_id)
        .ok_or_else(|| anyhow::anyhow!("No workspace ID specified and no default workspace set"))
}

pub async fn load_config() -> Result<Option<Config>> {
    load_config_path(None).await
}

pub async fn load_config_path(path: Option<&str>) -> Result<Option<Config>> {
    let config_path = config_path(path)?;
    let file = match tokio::fs::File::open(&config_path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to open .psyslop/secret file: {}",
                e
            ));
        }
    };
    let mut reader = tokio::io::BufReader::new(file);
    let mut contents = String::new();
    reader
        .read_to_string(&mut contents)
        .await
        .context("Failed to read .psyslop/secret")?;
    let auth =
        serde_json::from_str::<Config>(&contents).context("Failed to parse secret file as Auth")?;
    Ok(Some(auth))
}
