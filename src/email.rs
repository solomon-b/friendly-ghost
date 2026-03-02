use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::config::EmailConfig;
use crate::error::AppError;

/// Send an email report via SMTP with STARTTLS.
pub fn send_report(config: EmailConfig, subject: String, body: String) -> Result<(), AppError> {
    let password = config.password.ok_or_else(|| {
        AppError::Email(
            "SMTP password not set (use FRIENDLY_GHOST_SMTP_PASSWORD env var)".into(),
        )
    })?;

    let mut message_builder = Message::builder()
        .from(
            config
                .from
                .parse()
                .map_err(|e| AppError::Email(format!("invalid from address: {e}").into()))?,
        )
        .subject(subject);

    for recipient in &config.to {
        message_builder = message_builder.to(recipient
            .parse()
            .map_err(|e| AppError::Email(format!("invalid to address '{recipient}': {e}").into()))?);
    }

    let email = message_builder
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| AppError::Email(format!("failed to build email: {e}").into()))?;

    let creds = Credentials::new(config.username, password);

    let mailer = SmtpTransport::starttls_relay(&config.smtp_host)
        .map_err(|e| AppError::Email(format!("failed to create SMTP transport: {e}").into()))?
        .port(config.smtp_port)
        .credentials(creds)
        .build();

    mailer
        .send(&email)
        .map_err(|e| AppError::Email(format!("failed to send email: {e}").into()))?;

    Ok(())
}
