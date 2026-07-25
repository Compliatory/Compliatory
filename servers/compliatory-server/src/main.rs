#![forbid(unsafe_code)]

use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use compliatory_application::{AuthContext, RegulatoryService};
use compliatory_mcp::{CompliatoryMcpServer, run_stdio};
use compliatory_sqlite::SqliteRepository;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(about = "Compliatory MCP regulatory reference server")]
struct Args {
    #[arg(long, env = "COMPLIATORY_DATA_DIR", default_value = ".compliatory")]
    data_dir: PathBuf,

    #[arg(long, env = "COMPLIATORY_TENANT", default_value = "local")]
    tenant: String,

    #[arg(
        long,
        env = "COMPLIATORY_SUBJECT",
        default_value = "service:local-stdio"
    )]
    subject: String,

    #[arg(long, env = "COMPLIATORY_CURSOR_KEY")]
    cursor_key: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    let args = Args::parse();
    let repository = Arc::new(SqliteRepository::open(&args.data_dir)?);
    repository.ensure_tenant(&args.tenant)?;
    let key = cursor_key(&args)?;
    let service = RegulatoryService::new(repository, key)?;
    let auth = AuthContext::local_service(&args.tenant, &args.subject);
    info!(
        tenant = args.tenant,
        subject = args.subject,
        "starting Compliatory over STDIO"
    );
    run_stdio(CompliatoryMcpServer::new(service, auth)).await
}

fn cursor_key(args: &Args) -> anyhow::Result<Vec<u8>> {
    if let Some(key) = &args.cursor_key {
        if key.len() < 32 {
            anyhow::bail!("COMPLIATORY_CURSOR_KEY must contain at least 32 bytes");
        }
        return Ok(key.as_bytes().to_vec());
    }
    // Local-only deterministic key. Hosted/HTTP composition must require a secret key.
    let fallback = format!(
        "compliatory-local-cursor-key-v1:{}:{}",
        args.tenant, args.subject
    );
    Ok(fallback.into_bytes())
}
