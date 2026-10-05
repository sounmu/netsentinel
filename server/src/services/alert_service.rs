use reqwest::Client;

use crate::db::DbPool;
use crate::repositories::notification_channels_repo::{self, ChannelType, NotificationChannelRow};
use crate::services::url_validator;

/// How long to wait for each validated SMTP address before trying the next.
const SMTP_CONNECT_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The first of `addrs` that accepts a TCP connection within `timeout`.
///
/// The SMTP transport is pointed at one validated IP (so DNS cannot be
/// re-resolved to an internal address), which means it no longer walks the
/// address list itself. Without this, a host with no IPv6 route fails every
/// mail whose relay lists an AAAA record first.
async fn first_reachable(
    addrs: &[std::net::SocketAddr],
    timeout: std::time::Duration,
) -> Option<std::net::SocketAddr> {
    for addr in addrs {
        if let Ok(Ok(_)) = tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await
        {
            return Some(*addr);
        }
    }
    None
}

// ──────────────────────────────────────────────
// Multi-channel alert delivery
// ──────────────────────────────────────────────

/// Send an alert message to all enabled notification channels from the DB.
pub async fn send_alert(client: &Client, pool: &DbPool, message: &str) {
    let channels = notification_channels_repo::get_enabled(pool)
        .await
        .unwrap_or_default();
    for channel in &channels {
        match channel.channel_type {
            ChannelType::Discord => {
                if let Some(url) = channel.config.get("webhook_url").and_then(|v| v.as_str()) {
                    send_discord(client, url, message).await;
                }
            }
            ChannelType::Slack => {
                if let Some(url) = channel.config.get("webhook_url").and_then(|v| v.as_str()) {
                    send_slack(client, url, message).await;
                }
            }
            ChannelType::Email => {
                send_email(&channel.config, message).await;
            }
            ChannelType::Teams => {
                if let Some(url) = channel.config.get("webhook_url").and_then(|v| v.as_str()) {
                    send_teams(client, url, message).await;
                }
            }
            ChannelType::Telegram => {
                send_telegram(client, &channel.config, message).await;
            }
            ChannelType::Webhook => {
                if let Some(url) = channel.config.get("webhook_url").and_then(|v| v.as_str()) {
                    send_generic_webhook(client, url, message).await;
                }
            }
        }
    }
}

/// Test a specific notification channel by sending a test message
pub async fn test_channel(client: &Client, channel: &NotificationChannelRow) -> Result<(), String> {
    let test_msg = format!(
        "🔔 **[Test]** Notification channel `{}` is working!",
        channel.name
    );

    match channel.channel_type {
        ChannelType::Discord => {
            let url = channel
                .config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .ok_or("Missing webhook_url in config")?;
            if send_discord(client, url, &test_msg).await {
                Ok(())
            } else {
                Err("Discord webhook request failed".to_string())
            }
        }
        ChannelType::Slack => {
            let url = channel
                .config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .ok_or("Missing webhook_url in config")?;
            if send_slack(client, url, &test_msg).await {
                Ok(())
            } else {
                Err("Slack webhook request failed".to_string())
            }
        }
        ChannelType::Email => {
            send_email(&channel.config, &test_msg).await;
            Ok(())
        }
        ChannelType::Teams => {
            let url = channel
                .config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .ok_or("Missing webhook_url in config")?;
            if send_teams(client, url, &test_msg).await {
                Ok(())
            } else {
                Err("Teams webhook request failed".to_string())
            }
        }
        ChannelType::Telegram => {
            if send_telegram(client, &channel.config, &test_msg).await {
                Ok(())
            } else {
                Err("Telegram request failed".to_string())
            }
        }
        ChannelType::Webhook => {
            let url = channel
                .config
                .get("webhook_url")
                .and_then(|v| v.as_str())
                .ok_or("Missing webhook_url in config")?;
            if send_generic_webhook(client, url, &test_msg).await {
                Ok(())
            } else {
                Err("Generic webhook request failed".to_string())
            }
        }
    }
}

// ──────────────────────────────────────────────
// Channel implementations
// ──────────────────────────────────────────────

async fn send_discord(client: &Client, webhook_url: &str, message: &str) -> bool {
    // Defense-in-depth: re-validate URL at runtime to prevent SSRF via DNS rebinding
    if let Err(e) = url_validator::validate_url(webhook_url, &["https"]).await {
        tracing::error!(channel = "discord", err = %e, "⚠️ [Alert] Webhook URL failed SSRF validation");
        return false;
    }

    let body = serde_json::json!({ "content": message });

    match client.post(webhook_url).json(&body).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!(channel = "discord", "🔔 [Alert Sent]");
                true
            } else {
                tracing::error!(channel = "discord", status = %response.status(), "⚠️ [Alert Failed]");
                false
            }
        }
        Err(e) => {
            tracing::error!(channel = "discord", err = ?e, "⚠️ [Alert Error]");
            false
        }
    }
}

async fn send_slack(client: &Client, webhook_url: &str, message: &str) -> bool {
    // Defense-in-depth: re-validate URL at runtime to prevent SSRF via DNS rebinding
    if let Err(e) = url_validator::validate_url(webhook_url, &["https"]).await {
        tracing::error!(channel = "slack", err = %e, "⚠️ [Alert] Webhook URL failed SSRF validation");
        return false;
    }

    // Slack uses markdown-like formatting (mrkdwn), convert Discord markdown
    let slack_text = message.replace("**", "*"); // Discord bold (**) → Slack bold (*)

    let body = serde_json::json!({
        "text": slack_text,
        "mrkdwn": true,
    });

    match client.post(webhook_url).json(&body).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!(channel = "slack", "🔔 [Alert Sent]");
                true
            } else {
                tracing::error!(channel = "slack", status = %response.status(), "⚠️ [Alert Failed]");
                false
            }
        }
        Err(e) => {
            tracing::error!(channel = "slack", err = ?e, "⚠️ [Alert Error]");
            false
        }
    }
}

async fn send_teams(client: &Client, webhook_url: &str, message: &str) -> bool {
    if let Err(e) = url_validator::validate_url(webhook_url, &["https"]).await {
        tracing::error!(channel = "teams", err = %e, "⚠️ [Alert] Webhook URL failed SSRF validation");
        return false;
    }

    let body = serde_json::json!({ "text": message.replace("**", "") });

    match client.post(webhook_url).json(&body).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!(channel = "teams", "🔔 [Alert Sent]");
                true
            } else {
                tracing::error!(channel = "teams", status = %response.status(), "⚠️ [Alert Failed]");
                false
            }
        }
        Err(e) => {
            tracing::error!(channel = "teams", err = ?e, "⚠️ [Alert Error]");
            false
        }
    }
}

async fn send_telegram(client: &Client, config: &serde_json::Value, message: &str) -> bool {
    let bot_token = config
        .get("bot_token")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let chat_id = config.get("chat_id").and_then(|v| v.as_str()).unwrap_or("");
    if bot_token.is_empty() || chat_id.is_empty() {
        tracing::warn!(
            channel = "telegram",
            "⚠️ [Telegram] Missing bot_token or chat_id"
        );
        return false;
    }

    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": message.replace("**", "").replace('`', ""),
        "disable_web_page_preview": true,
    });

    match client.post(url).json(&body).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!(channel = "telegram", "🔔 [Alert Sent]");
                true
            } else {
                tracing::error!(channel = "telegram", status = %response.status(), "⚠️ [Alert Failed]");
                false
            }
        }
        Err(e) => {
            tracing::error!(channel = "telegram", err = ?e, "⚠️ [Alert Error]");
            false
        }
    }
}

async fn send_generic_webhook(client: &Client, webhook_url: &str, message: &str) -> bool {
    if let Err(e) = url_validator::validate_url(webhook_url, &["https"]).await {
        tracing::error!(channel = "webhook", err = %e, "⚠️ [Alert] Webhook URL failed SSRF validation");
        return false;
    }

    let body = serde_json::json!({
        "source": "netsentinel",
        "text": message,
    });

    match client.post(webhook_url).json(&body).send().await {
        Ok(response) => {
            if response.status().is_success() {
                tracing::info!(channel = "webhook", "🔔 [Alert Sent]");
                true
            } else {
                tracing::error!(channel = "webhook", status = %response.status(), "⚠️ [Alert Failed]");
                false
            }
        }
        Err(e) => {
            tracing::error!(channel = "webhook", err = ?e, "⚠️ [Alert Error]");
            false
        }
    }
}

async fn send_email(config: &serde_json::Value, message: &str) {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let smtp_host = config
        .get("smtp_host")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let smtp_port = config
        .get("smtp_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(587) as u16;
    let smtp_user = config
        .get("smtp_user")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let smtp_pass = config
        .get("smtp_pass")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let from = config.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let to = config.get("to").and_then(|v| v.as_str()).unwrap_or("");

    if smtp_host.is_empty() || from.is_empty() || to.is_empty() {
        tracing::warn!(
            channel = "email",
            "⚠️ [Email] Missing required config (smtp_host, from, to)"
        );
        return;
    }

    // Runtime SSRF re-check. Handler-time validation is not enough: if the
    // blocklist grew (new private-IP range, new disallowed port) after a
    // channel was saved, or the DNS record for `smtp_host` changed to point
    // at an internal IP, the previously-valid config would otherwise be
    // honored. Also guards against raw DB writes that skip the handler.
    let smtp_addr = match crate::services::url_validator::resolve_public_host(&format!(
        "{smtp_host}:{smtp_port}"
    ))
    .await
    {
        // A relay usually resolves to several addresses (IPv6 and IPv4, or a
        // pool). Dial them in order and use the first that answers; falling
        // back to the first address keeps the SMTP error meaningful when
        // none do.
        Ok(addrs) if !addrs.is_empty() => first_reachable(&addrs, SMTP_CONNECT_PROBE_TIMEOUT)
            .await
            .unwrap_or(addrs[0]),
        Ok(_) => return,
        Err(e) => {
            tracing::error!(
                channel = "email",
                smtp_host,
                smtp_port,
                err = %e,
                "🚫 [Email] SSRF block — refusing to connect"
            );
            return;
        }
    };

    if smtp_user.is_empty() || smtp_pass.is_empty() {
        tracing::warn!(
            channel = "email",
            "⚠️ [Email] SMTP credentials are empty — authentication will likely fail"
        );
    }

    // Strip markdown formatting for plain-text email
    let plain_text = message.replace("**", "").replace('`', "");

    let Ok(from_addr) = from.parse() else {
        tracing::error!(channel = "email", from, "⚠️ [Email] Invalid 'from' address");
        return;
    };
    let Ok(to_addr) = to.parse() else {
        tracing::error!(channel = "email", to, "⚠️ [Email] Invalid 'to' address");
        return;
    };

    let email = match Message::builder()
        .from(from_addr)
        .to(to_addr)
        .subject("NetSentinel Alert")
        .body(plain_text)
    {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(channel = "email", err = ?e, "⚠️ [Email] Failed to build message");
            return;
        }
    };

    let creds = Credentials::new(smtp_user.to_string(), smtp_pass.to_string());

    // Same as `starttls_relay(smtp_host)`, except the TCP connection goes to
    // the address validated above instead of resolving `smtp_host` a second
    // time. The certificate is still verified against the hostname.
    let tls = match lettre::transport::smtp::client::TlsParameters::new(smtp_host.to_string()) {
        Ok(params) => lettre::transport::smtp::client::Tls::Required(params),
        Err(e) => {
            tracing::error!(channel = "email", err = ?e, "⚠️ [Email] Failed to create SMTP transport");
            return;
        }
    };
    let mailer =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(smtp_addr.ip().to_string())
            .port(smtp_port)
            .tls(tls)
            .credentials(creds)
            .build();

    match mailer.send(email).await {
        Ok(_) => tracing::info!(channel = "email", to = %to, "🔔 [Alert Sent]"),
        Err(e) => tracing::error!(channel = "email", err = ?e, "⚠️ [Alert Failed]"),
    }
}

#[cfg(test)]
mod reachability_tests {
    use super::first_reachable;
    use std::net::SocketAddr;
    use std::time::Duration;

    /// An address nothing listens on: bind an ephemeral port, then free it.
    async fn closed_addr() -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    }

    #[tokio::test]
    async fn skips_unreachable_addresses_and_uses_the_next_one() {
        let dead = closed_addr().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let live = listener.local_addr().unwrap();

        // The old behaviour dialled only the first entry and gave up.
        let picked = first_reachable(&[dead, live], Duration::from_secs(2)).await;
        assert_eq!(picked, Some(live));
    }

    #[tokio::test]
    async fn prefers_the_first_address_when_it_answers() {
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addrs = [first.local_addr().unwrap(), second.local_addr().unwrap()];
        assert_eq!(
            first_reachable(&addrs, Duration::from_secs(2)).await,
            Some(addrs[0])
        );
    }

    #[tokio::test]
    async fn reports_none_when_nothing_answers() {
        let dead = closed_addr().await;
        assert_eq!(first_reachable(&[dead], Duration::from_secs(2)).await, None);
    }
}
