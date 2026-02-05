use crate::args::PostgresArgs;
use anyhow::{Result, bail};
use deadpool_postgres::{Config as PgConfig, ManagerConfig, Pool, RecyclingMethod};
use postgres::{Config as PostgresConfig, NoTls};
use rustls::pki_types::CertificateDer;
use std::env;
use tempfile::NamedTempFile;

pub async fn create_pool(pg_args: PostgresArgs) -> Pool {
    let mut pg_cfg = PostgresConfig::new();
    pg_cfg.host(&pg_args.postgres_host);
    pg_cfg.port(pg_args.postgres_port);
    pg_cfg.dbname(&pg_args.postgres_database);
    pg_cfg.user(&pg_args.postgres_username);
    if let Some(ref pw) = pg_args.postgres_password {
        pg_cfg.password(pw);
    } else if let Ok(pw) = env::var("POSTGRES_PASSWORD") {
        pg_cfg.password(pw);
    }
    let mut _ca_tempfile = None;
    let mut tls_connector = None;
    if let Some(ref ca_cert) = pg_args.postgres_ca_cert {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");
        use std::io::Write;
        file.write_all(ca_cert.as_bytes())
            .expect("Failed to write CA certificate");
        let cert_bytes = std::fs::read(file.path()).expect("Failed to read CA certificate");
        let extra_roots = parse_ca_certs(&cert_bytes).expect("Failed to parse CA certificate");
        tls_connector =
            Some(crate::make_rustls(extra_roots).expect("Failed to create Rustls connector"));
        _ca_tempfile = Some(file); // Keep tempfile alive
    }
    let mut pg_pool_cfg = PgConfig::new();
    pg_pool_cfg.dbname = Some(pg_args.postgres_database);
    pg_pool_cfg.host = Some(pg_args.postgres_host);
    pg_pool_cfg.port = Some(pg_args.postgres_port);
    pg_pool_cfg.user = Some(pg_args.postgres_username);
    pg_pool_cfg.password = pg_args.postgres_password;
    pg_pool_cfg.manager = Some(ManagerConfig {
        recycling_method: RecyclingMethod::Fast,
    });
    if let Some(tls) = tls_connector {
        pg_pool_cfg
            .create_pool(Some(deadpool_postgres::Runtime::Tokio1), tls)
            .expect("Failed to create Postgres pool")
    } else {
        pg_pool_cfg
            .create_pool(Some(deadpool_postgres::Runtime::Tokio1), NoTls)
            .expect("Failed to create Postgres pool")
    }
}

fn parse_ca_certs(bytes: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    // If it's PEM, decode all the certs; otherwise treat as raw DER
    if bytes.starts_with(b"-----BEGIN") {
        let mut rd: &[u8] = bytes;
        let mut out = Vec::new();
        for item in rustls_pemfile::read_all(&mut rd) {
            let item = item.map_err(|e| anyhow::anyhow!("failed to parse PEM bundle: {}", e))?;
            if let rustls_pemfile::Item::X509Certificate(der) = item {
                out.push(der);
            }
        }
        if out.is_empty() {
            bail!("no X509 certificates found in provided PEM");
        }
        Ok(out)
    } else {
        Ok(vec![CertificateDer::from(bytes.to_vec())])
    }
}

pub fn strip_sql_comments(input: &str) -> String {
    let mut output = String::new();
    for line in input.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--") || trimmed.is_empty() {
            continue;
        }
        if let Some(pos) = line.find("--") {
            output.push_str(&line[..pos]);
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}
