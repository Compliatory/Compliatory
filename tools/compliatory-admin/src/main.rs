#![forbid(unsafe_code)]

use std::{
    fs,
    path::PathBuf,
    process::{Command, ExitCode},
};

use anyhow::Context;
use clap::{Parser, Subcommand};
use compliatory_admin::seed_synthetic_fixtures;
use compliatory_pdf::{
    CommandScanner, IngestionManifest, IngestionService, SecurityScanner, SyntheticFixtureScanner,
};
use compliatory_sqlite::SqliteRepository;

#[derive(Debug, Parser)]
#[command(about = "Human-controlled Compliatory corpus administration")]
struct Args {
    #[arg(long, env = "COMPLIATORY_DATA_DIR", default_value = ".compliatory")]
    data_dir: PathBuf,

    #[arg(long, env = "COMPLIATORY_TENANT", default_value = "local")]
    tenant: String,

    #[command(subcommand)]
    command: AdminCommand,
}

#[derive(Debug, Subcommand)]
enum AdminCommand {
    /// Install original, non-normative fixtures for all pilot families.
    SeedFixtures,

    /// Inspect, scan and quarantine a PDF, then extract it in a worker process.
    Ingest {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        pdf: PathBuf,
        #[arg(long)]
        scanner_command: Option<PathBuf>,
        #[arg(long, hide = true)]
        synthetic_fixture_scanner: bool,
    },

    /// Print the immutable review bundle for human comparison with the source PDF.
    Review {
        #[arg(long)]
        ingestion_id: String,
    },

    /// Approve the exact review digest after human comparison.
    Approve {
        #[arg(long)]
        ingestion_id: String,
        #[arg(long)]
        review_digest: String,
        #[arg(long)]
        approver: String,
    },

    /// Publish an approved ingestion as a new immutable corpus version.
    Publish {
        #[arg(long)]
        ingestion_id: String,
    },

    #[command(hide = true)]
    ExtractWorker {
        #[arg(long)]
        ingestion_id: String,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let repository = SqliteRepository::open(&args.data_dir)?;
    repository.ensure_tenant(&args.tenant)?;
    let ingestion = IngestionService::new(&repository);
    match args.command {
        AdminCommand::SeedFixtures => {
            seed_synthetic_fixtures(&repository, &args.tenant)?;
            println!(
                "{}",
                serde_json::json!({
                    "tenant": args.tenant,
                    "status": "fixtures_seeded",
                    "warning": "synthetic non-normative test material"
                })
            );
        }
        AdminCommand::Ingest {
            manifest,
            pdf,
            scanner_command,
            synthetic_fixture_scanner,
        } => {
            let manifest: IngestionManifest = serde_json::from_slice(
                &fs::read(&manifest)
                    .with_context(|| format!("failed to read {}", manifest.display()))?,
            )?;
            let scanner: Box<dyn SecurityScanner> = match (
                scanner_command,
                synthetic_fixture_scanner,
            ) {
                (Some(command), false) => Box::new(CommandScanner::new(command)),
                (None, true) => Box::new(SyntheticFixtureScanner),
                (Some(_), true) => anyhow::bail!("select exactly one security scanner"),
                (None, false) => anyhow::bail!(
                    "--scanner-command is required for real sources; the hidden fixture scanner is test-only"
                ),
            };
            let record = ingestion.ingest(&args.tenant, &manifest, &pdf, scanner.as_ref())?;
            println!("{}", serde_json::to_string_pretty(&record)?);
            run_extraction_worker(&args.data_dir, &args.tenant, &record.ingestion_id)?;
        }
        AdminCommand::Review { ingestion_id } => {
            let record = ingestion.record(&args.tenant, &ingestion_id)?;
            let review = repository
                .tenant_dir(&args.tenant)?
                .join("quarantine")
                .join(&ingestion_id)
                .join("review.json");
            println!("{}", serde_json::to_string_pretty(&record)?);
            if review.exists() {
                println!("{}", String::from_utf8(fs::read(review)?)?);
            }
        }
        AdminCommand::Approve {
            ingestion_id,
            review_digest,
            approver,
        } => {
            let record =
                ingestion.approve(&args.tenant, &ingestion_id, &review_digest, &approver)?;
            println!("{}", serde_json::to_string_pretty(&record)?);
        }
        AdminCommand::Publish { ingestion_id } => {
            let record = ingestion.publish(&args.tenant, &ingestion_id)?;
            println!("{}", serde_json::to_string_pretty(&record)?);
        }
        AdminCommand::ExtractWorker { ingestion_id } => {
            let record = ingestion.extract_worker(&args.tenant, &ingestion_id)?;
            println!("{}", serde_json::to_string_pretty(&record)?);
        }
    }
    Ok(())
}

fn run_extraction_worker(
    data_dir: &std::path::Path,
    tenant: &str,
    ingestion_id: &str,
) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let status = Command::new(executable)
        .arg("--data-dir")
        .arg(data_dir)
        .arg("--tenant")
        .arg(tenant)
        .arg("extract-worker")
        .arg("--ingestion-id")
        .arg(ingestion_id)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .status()
        .context("failed to start the extraction worker")?;
    if !status.success() {
        anyhow::bail!("extraction worker failed; source remains quarantined");
    }
    Ok(())
}
