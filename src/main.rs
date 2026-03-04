mod config;
mod email;
mod error;
mod filter;
mod journal;
mod llm;
mod report;

use std::path::PathBuf;

use clap::Parser;

use error::AppError;
use journal::JournalResult;

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
    let cursor = journal::read_cursor(&cfg.state.cursor_file)?;

    let matcher = cfg
        .journal
        .unit_matcher
        .as_ref()
        .expect("unit_matcher is always built by config::load");

    match journal::query_journal(cursor.as_deref())? {
        JournalResult::FirstRun(Some(baseline_cursor)) => {
            journal::save_cursor(&cfg.state.cursor_file, &baseline_cursor)?;
            eprintln!("first run: cursor saved, will report new entries on next run");
        }
        JournalResult::FirstRun(None) => {
            eprintln!("first run: journal is empty, nothing to save");
        }
        JournalResult::Entries(raw_entries) => {
            // Save cursor from last raw entry before filtering, so we don't re-scan
            if let Some(last) = raw_entries.last() {
                journal::save_cursor(&cfg.state.cursor_file, &last.cursor)?;
            }

            let entries =
                filter::filter_entries(raw_entries, matcher, cfg.journal.priority);

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
                    let body = report::format_report(&entries, &hostname);
                    let subject = report::format_subject(
                        &cfg.email.subject_prefix,
                        entries.len(),
                        &hostname,
                    );
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
