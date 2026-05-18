mod config;
mod email;
mod error;
mod filter;
mod journal;
mod llm;
mod loki;
mod report;
mod source;

use std::path::PathBuf;

use clap::Parser;

use error::AppError;
use source::LogResult;

#[derive(Parser)]
#[command(
    name = "friendly-ghost",
    about = "Systemd journal monitor with email alerts"
)]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: PathBuf,

    /// Print report to stdout instead of sending email
    #[arg(long)]
    dry_run: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    let cfg = config::load(&cli.config, config::EnvOverrides::from_env())?;

    let matcher = cfg
        .filter
        .unit_matcher
        .as_ref()
        .expect("unit_matcher is always built by config::load");

    match source::query(&cfg)? {
        LogResult::FirstRun(Some(_)) => {
            eprintln!("first run: cursor saved, will report new entries on next run");
        }
        LogResult::FirstRun(None) => {
            eprintln!("first run: log source is empty, nothing to save");
        }
        LogResult::Entries(raw_entries) => {
            let entries = filter::filter_entries(
                raw_entries,
                matcher,
                cfg.filter.priority,
                cfg.filter.ignore_matcher.as_ref(),
            );

            if entries.is_empty() {
                return Ok(());
            }

            let hostname = hostname();

            let (subject, body) = match &cfg.llm {
                Some(llm_config) => {
                    match llm::analyze(&entries, &hostname, llm_config)? {
                        llm::LlmVerdict::NoIssues => {
                            eprintln!("LLM analysis: no issues found");
                            return Ok(());
                        }
                        llm::LlmVerdict::Alert { subject, body } => (subject, body),
                    }
                }
                None => {
                    let body = report::format_report(&entries);
                    let subject = report::format_subject(&cfg.email.subject_prefix, &entries);
                    (subject, body)
                }
            };

            if cli.dry_run {
                println!("{subject}\n\n{body}");
            } else {
                email::send_report(cfg.email, subject, body)?;
                eprintln!("sent alert with {} entries", entries.len());
            }
        }
    }

    Ok(())
}

fn hostname() -> String {
    match std::fs::read_to_string("/etc/hostname") {
        Ok(mut s) => {
            let end = s.trim_end().len();
            s.truncate(end);
            if s.is_empty() { "unknown".to_string() } else { s }
        }
        Err(_) => "unknown".to_string(),
    }
}
