//! Credential mail delivery. SMTP is always encrypted; local sinks are development-only.
use std::{fmt, time::Duration};

use clap::{Args, ValueEnum};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};

use crate::error::{ApiError, ApiResult};

const DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Default, Clone, Copy, ValueEnum)]
pub enum SmtpTls {
    /// Require STARTTLS before sending credentials or mail (usually port 587).
    #[default]
    Starttls,
    /// Encrypt from connection establishment (usually port 465).
    Implicit,
}

#[derive(Clone, Args)]
pub struct MailConfig {
    #[arg(long, env = "FINPLAN_SMTP_HOST")]
    pub smtp_host: Option<String>,
    #[arg(long, env = "FINPLAN_SMTP_PORT", default_value_t = 587)]
    pub smtp_port: u16,
    #[arg(long, env = "FINPLAN_SMTP_TLS", value_enum, default_value = "starttls")]
    pub smtp_tls: SmtpTls,
    #[arg(long, env = "FINPLAN_SMTP_USERNAME", hide_env_values = true)]
    pub smtp_username: Option<String>,
    #[arg(long, env = "FINPLAN_SMTP_PASSWORD", hide_env_values = true)]
    pub smtp_password: Option<String>,
    /// Verified sender, for example FinPlan <accounts@example.com>.
    #[arg(long, env = "FINPLAN_MAIL_FROM")]
    pub mail_from: Option<String>,
    /// Public frontend origin used in mail, for example https://finplan.example.
    #[arg(long, env = "FINPLAN_PUBLIC_URL")]
    pub public_url: Option<String>,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            smtp_host: None,
            smtp_port: 587,
            smtp_tls: SmtpTls::Starttls,
            smtp_username: None,
            smtp_password: None,
            mail_from: None,
            public_url: None,
        }
    }
}

// ServerConfig derives Debug: never let that expose SMTP credentials.
impl fmt::Debug for MailConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MailConfig")
            .field("configured", &self.available())
            .finish_non_exhaustive()
    }
}

impl MailConfig {
    pub fn available(&self) -> bool {
        self.smtp_host.is_some()
            && self.smtp_username.is_some()
            && self.smtp_password.is_some()
            && self.mail_from.is_some()
            && self.public_url.is_some()
    }

    pub fn validate(&self, hosted: bool, local_sink: Option<&str>) -> Result<(), String> {
        if hosted && local_sink.is_some() {
            return Err("hosted mode cannot use a local mail sink".into());
        }
        let values = [
            &self.smtp_host,
            &self.smtp_username,
            &self.smtp_password,
            &self.mail_from,
            &self.public_url,
        ];
        if values.iter().all(|value| value.is_none()) {
            return Ok(());
        }
        if values
            .iter()
            .any(|value| value.as_ref().is_none_or(|value| value.trim().is_empty()))
            || self.smtp_port == 0
        {
            return Err(
                "SMTP requires host, nonzero port, username, password, sender, and public URL"
                    .into(),
            );
        }
        if local_sink.is_some() {
            return Err("configure SMTP or a local mail sink, not both".into());
        }
        self.mail_from
            .as_ref()
            .unwrap()
            .parse::<Mailbox>()
            .map_err(|_| "SMTP sender must be a valid mailbox".to_string())?;
        let public_url = self.public_url.as_ref().unwrap();
        let uri = public_url
            .parse::<axum::http::Uri>()
            .map_err(|_| "mail public URL must be an HTTPS frontend origin".to_string())?;
        if uri.scheme_str() != Some("https")
            || uri.authority().is_none()
            || uri
                .authority()
                .is_some_and(|a| a.as_str().contains('@') || a.as_str().contains('*'))
            || uri.path() != "/"
            || uri.query().is_some()
            || public_url.contains('#')
            || public_url.trim() != public_url
        {
            return Err("mail public URL must be an HTTPS frontend origin".into());
        }
        self.transport()
            .map_err(|_| "SMTP host must be a valid TLS server name".to_string())?;
        Ok(())
    }

    fn transport(&self) -> ApiResult<AsyncSmtpTransport<Tokio1Executor>> {
        let host = self.smtp_host.as_deref().ok_or_else(delivery_error)?;
        let builder = match self.smtp_tls {
            SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
            SmtpTls::Implicit => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        }
        .map_err(|_| delivery_error())?;
        Ok(builder
            .port(self.smtp_port)
            .timeout(Some(DELIVERY_TIMEOUT))
            .credentials(Credentials::new(
                self.smtp_username.clone().ok_or_else(delivery_error)?,
                self.smtp_password.clone().ok_or_else(delivery_error)?,
            ))
            .build())
    }

    pub async fn deliver(
        &self,
        local_sink: Option<&str>,
        email: &str,
        purpose: &str,
        token: &str,
    ) -> ApiResult<()> {
        if let Some(directory) = local_sink {
            return write_local(directory, email, purpose, token);
        }
        let message = self.message(email, purpose, token)?;
        let transport = self.transport()?;
        tokio::time::timeout(DELIVERY_TIMEOUT, transport.send(message))
            .await
            .map_err(|_| delivery_error())?
            .map_err(|_| delivery_error())?;
        Ok(())
    }

    fn message(&self, email: &str, purpose: &str, token: &str) -> ApiResult<Message> {
        let (subject, instructions) = match purpose {
            "reset" => (
                "Reset your FinPlan password",
                "On the sign-in screen, select Forgot password, then I have a recovery token. Paste the token below and choose a new password.",
            ),
            "verify" => (
                "Verify your FinPlan email",
                "Sign in, open Account, and paste the token below into Email verification.",
            ),
            _ => return Err(delivery_error()),
        };
        let url = self.public_url.as_deref().ok_or_else(delivery_error)?;
        let body = format!(
            "{subject}\n\nOpen {url}\n\n{instructions}\n\n{token}\n\nThis token expires in 30 minutes and can only be used once. If you did not request this email, you can ignore it. Never share this token.\n"
        );
        Message::builder()
            .from(
                self.mail_from
                    .as_deref()
                    .ok_or_else(delivery_error)?
                    .parse()
                    .map_err(|_| delivery_error())?,
            )
            .to(email.parse().map_err(|_| delivery_error())?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)
            .map_err(|_| delivery_error())
    }
}

fn delivery_error() -> ApiError {
    // Provider errors can include recipient addresses and other private data.
    ApiError::internal("credential email delivery failed")
}

fn write_local(directory: &str, email: &str, purpose: &str, token: &str) -> ApiResult<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    std::fs::create_dir_all(directory).map_err(|_| delivery_error())?;
    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| delivery_error())?;
    let path = std::path::Path::new(directory).join(format!("{}.json", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| delivery_error())?;
    let message = serde_json::json!({"to": email, "purpose": purpose, "token": token, "expires_in_seconds": 1800});
    file.write_all(message.to_string().as_bytes())
        .map_err(|_| delivery_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> MailConfig {
        MailConfig {
            smtp_host: Some("smtp.example.com".into()),
            smtp_username: Some("private-username".into()),
            smtp_password: Some("private-password".into()),
            mail_from: Some("FinPlan <accounts@example.com>".into()),
            public_url: Some("https://finplan.example.com".into()),
            ..Default::default()
        }
    }

    #[test]
    fn configuration_requires_complete_secure_delivery_and_redacts_secrets() {
        let config = configured();
        assert!(config.validate(true, None).is_ok());
        assert!(config.validate(false, Some("/tmp/mail")).is_err());
        assert!(!format!("{config:?}").contains("private"));
        let mut incomplete = config.clone();
        incomplete.smtp_password = None;
        assert!(incomplete.validate(true, None).is_err());
        for url in [
            "http://example.com",
            "https://x@example.com",
            "https://example.com/path",
            "https://example.com?token=x",
            "https://example.com/#fragment",
        ] {
            let mut invalid = config.clone();
            invalid.public_url = Some(url.into());
            assert!(invalid.validate(true, None).is_err(), "{url}");
        }
        assert!(
            MailConfig::default()
                .validate(true, Some("/tmp/mail"))
                .is_err()
        );
    }

    #[test]
    fn mail_contains_manual_token_instructions_without_credentials_in_links() {
        for purpose in ["reset", "verify"] {
            let bytes = configured()
                .message("user@example.com", purpose, "secret-token")
                .unwrap()
                .formatted();
            let text = String::from_utf8(bytes).unwrap();
            assert!(text.contains("secret-token"));
            assert!(text.contains("30 minutes"));
            assert!(text.contains("https://finplan.example.com"));
            assert!(!text.contains("?token="));
            assert!(!text.contains("private-password"));
        }
    }

    #[tokio::test]
    async fn local_mail_is_private_and_compatible_with_token_workflows() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mail");
        MailConfig::default()
            .deliver(path.to_str(), "user@example.com", "reset", "token")
            .await
            .unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let file = std::fs::read_dir(path)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let message: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(message["token"], "token");
        assert_eq!(message["expires_in_seconds"], 1800);
    }
}
