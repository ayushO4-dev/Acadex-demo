//! `notify.fanout` (architecture.md §11.2): a published notice reaches its
//! audience by email, as each person prefers (critical ones always). Every
//! delivery is recorded before the next, so a retried fan-out sends nobody
//! a second copy. In-app delivery is the notice itself; browsers hear of it
//! over the API's live stream.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
};

/// SMTP: Mailpit locally (`smtp://localhost:1025`), a relay on Google Cloud.
#[derive(Clone)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    /// `SMTP_URL` (e.g. `smtp://mailpit:1025`, or `smtps://user:pass@relay:465`)
    /// and `SMTP_FROM`. `None` if unset: notices then go out in-app only.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(url) = std::env::var("SMTP_URL").ok().filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        let from = std::env::var("SMTP_FROM")
            .unwrap_or_else(|_| "Acadex <no-reply@acadex.test>".to_owned());
        Ok(Some(Self {
            transport: AsyncSmtpTransport::<Tokio1Executor>::from_url(&url)?.build(),
            from: from.parse()?,
        }))
    }

    pub async fn send(
        &self,
        to_name: &str,
        to_email: &str,
        subject: &str,
        body: String,
    ) -> anyhow::Result<()> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(Mailbox::new(Some(to_name.to_owned()), to_email.parse()?))
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)?;
        self.transport.send(message).await?;
        Ok(())
    }
}

/// The email for a notice: plain text, as notices are.
pub fn compose(
    severity: &str,
    title: &str,
    body: &str,
    author: Option<&str>,
    app_url: &str,
) -> (String, String) {
    let tag = match severity {
        "critical" => "[Critical] ",
        "important" => "[Important] ",
        _ => "",
    };
    let subject = format!("{tag}{title}");
    let mut text = format!("{title}\n\n{body}\n\n");
    if let Some(author) = author {
        text.push_str(&format!("— {author}\n\n"));
    }
    if severity == "critical" {
        text.push_str("This notice is critical: please open Acadex and acknowledge it.\n");
    }
    text.push_str(&format!(
        "Read it on Acadex: {}/notices\n\nChoose which notices reach you by email on the Notices page.\n",
        app_url.trim_end_matches('/')
    ));
    (subject, text)
}
