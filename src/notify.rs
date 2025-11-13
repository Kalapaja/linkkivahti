//! Notification module for sending alerts about check failures

use crate::checker::CheckResult;
use worker::*;

/// Supported webhook service types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookService {
    /// Discord webhook (discord.com domain)
    Discord,
    /// Slack incoming webhook (hooks.slack.com domain)
    Slack,
    /// Zulip webhook (zulipchat.com or self-hosted)
    Zulip,
    /// Generic JSON webhook (fallback)
    Generic,
}

impl WebhookService {
    /// Detect service type from a webhook URL by inspecting its domain
    ///
    /// # Arguments
    /// * `url` - The webhook URL to analyze
    ///
    /// # Returns
    /// Detected WebhookService type
    pub fn from_url(url: &str) -> Self {
        let url_lower = url.to_lowercase();

        if url_lower.contains("discord.com") || url_lower.contains("discordapp.com") {
            Self::Discord
        } else if url_lower.contains("hooks.slack.com") || url_lower.contains("slack.com/api/") {
            Self::Slack
        } else if url_lower.contains("zulipchat.com") || url_lower.contains("/api/v1/messages") {
            Self::Zulip
        } else {
            Self::Generic
        }
    }

    /// Build a formatted webhook payload for this service type
    ///
    /// # Arguments
    /// * `result` - The check result to format
    /// * `timestamp` - ISO 8601 timestamp string
    ///
    /// # Returns
    /// JSON payload string appropriate for the service
    fn build_payload(&self, result: &CheckResult, timestamp: &str) -> String {
        match self {
            Self::Discord => Self::build_discord_payload(result, timestamp),
            Self::Slack => Self::build_slack_payload(result, timestamp),
            Self::Zulip => Self::build_zulip_payload(result, timestamp),
            Self::Generic => Self::build_generic_payload(result, timestamp),
        }
    }

    /// Build Discord webhook payload with embeds
    fn build_discord_payload(result: &CheckResult, timestamp: &str) -> String {
        format!(
            r#"{{
  "embeds": [{{
    "title": "🔗 Link Check Failed",
    "description": "**{}**",
    "color": 15158332,
    "fields": [
      {{"name": "Status", "value": "{}", "inline": true}},
      {{"name": "Time", "value": "{}", "inline": true}}
    ]
  }}]
}}"#,
            escape_json(&result.url),
            escape_json(&result.description()),
            timestamp
        )
    }

    /// Build Slack webhook payload with Block Kit
    fn build_slack_payload(result: &CheckResult, timestamp: &str) -> String {
        format!(
            r#"{{
  "blocks": [
    {{
      "type": "header",
      "text": {{
        "type": "plain_text",
        "text": "🔗 Link Check Failed"
      }}
    }},
    {{
      "type": "section",
      "fields": [
        {{
          "type": "mrkdwn",
          "text": "*URL:*\n{}"
        }},
        {{
          "type": "mrkdwn",
          "text": "*Status:*\n{}"
        }}
      ]
    }},
    {{
      "type": "context",
      "elements": [
        {{
          "type": "mrkdwn",
          "text": "Time: {} | Worker: linkkivahti"
        }}
      ]
    }}
  ]
}}"#,
            escape_json(&result.url),
            escape_json(&result.description()),
            timestamp
        )
    }

    /// Build Zulip webhook payload with markdown content
    fn build_zulip_payload(result: &CheckResult, timestamp: &str) -> String {
        let content = format!(
            "## 🔗 Link Check Failed\n\n**URL:** {}\n\n**Status:** {}\n\n**Time:** {}",
            result.url,
            result.description(),
            timestamp
        );

        format!(
            r#"{{
  "type": "stream",
  "to": "monitoring",
  "topic": "Link Checks",
  "content": "{}"
}}"#,
            escape_json(&content)
        )
    }

    /// Build generic JSON webhook payload
    fn build_generic_payload(result: &CheckResult, timestamp: &str) -> String {
        format!(
            r#"{{
  "timestamp": "{}",
  "status": "failure",
  "url": "{}",
  "error": "{}",
  "worker": "linkkivahti"
}}"#,
            timestamp,
            escape_json(&result.url),
            escape_json(&result.description())
        )
    }

    /// Get service-specific HTTP headers
    ///
    /// Returns a list of (header_name, header_value) tuples
    fn headers(&self) -> Vec<(&'static str, &'static str)> {
        match self {
            Self::Zulip => vec![("User-Agent", "linkkivahti/0.1.0")],
            _ => vec![],
        }
    }
}

impl std::fmt::Display for WebhookService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discord => write!(f, "Discord"),
            Self::Slack => write!(f, "Slack"),
            Self::Zulip => write!(f, "Zulip"),
            Self::Generic => write!(f, "Generic"),
        }
    }
}

impl std::str::FromStr for WebhookService {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "discord" => Ok(Self::Discord),
            "slack" => Ok(Self::Slack),
            "zulip" => Ok(Self::Zulip),
            "generic" => Ok(Self::Generic),
            _ => Err(()),
        }
    }
}

/// Send a notification about a failed check to the configured webhook
///
/// This function retrieves the webhook configuration from environment variables,
/// auto-detects the webhook service type (or uses an override), formats the
/// appropriate payload, and sends the notification.
///
/// # Arguments
/// * `env` - Worker environment to access WEBHOOK_URL secret and optional WEBHOOK_SERVICE override
/// * `result` - The check result to report
///
/// # Returns
/// * `Ok(())` if notification was sent successfully or webhook is not configured
/// * `Err` if webhook is configured but sending failed
pub async fn send_failure_notification(env: &Env, result: &CheckResult) -> Result<()> {
    // Get webhook URL from environment variable/secret
    let webhook_url = match env.secret("WEBHOOK_URL") {
        Ok(secret) => secret.to_string(),
        Err(_) => {
            console_log!("WEBHOOK_URL not configured, skipping notification");
            return Ok(());
        }
    };

    if webhook_url.is_empty() {
        console_log!("WEBHOOK_URL is empty, skipping notification");
        return Ok(());
    }

    // Detect webhook service type (with optional override)
    let service = detect_webhook_service(env, &webhook_url);
    console_log!(
        "Sending webhook notification for: {} via {}",
        result.url,
        service
    );

    // Build and send notification
    let timestamp = get_timestamp();
    let payload = service.build_payload(result, &timestamp);

    send_webhook(&webhook_url, &payload, service).await
}

/// Detect webhook service type from URL and environment variables
///
/// First checks for an explicit `WEBHOOK_SERVICE` environment variable override.
/// If not set, performs auto-detection based on the webhook URL domain.
///
/// # Arguments
/// * `env` - Worker environment to check for WEBHOOK_SERVICE override
/// * `webhook_url` - The webhook URL to analyze for auto-detection
///
/// # Returns
/// Detected or configured WebhookService type
fn detect_webhook_service(env: &Env, webhook_url: &str) -> WebhookService {
    use std::str::FromStr;

    // Check for explicit override via WEBHOOK_SERVICE environment variable
    if let Ok(override_service) = env.var("WEBHOOK_SERVICE") {
        let service_str = override_service.to_string();
        console_log!("WEBHOOK_SERVICE override detected: {}", service_str);

        match WebhookService::from_str(&service_str) {
            Ok(service) => return service,
            Err(_) => {
                console_log!(
                    "Unknown WEBHOOK_SERVICE value '{}', falling back to auto-detection",
                    service_str
                );
            }
        }
    }

    // Auto-detect from URL
    WebhookService::from_url(webhook_url)
}

/// Escape a string for safe inclusion in JSON
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

/// Send a webhook notification via HTTP POST
///
/// Sends a formatted payload to the webhook endpoint with appropriate headers
/// for the service type. Logs detailed error information if the request fails.
///
/// # Arguments
/// * `webhook_url` - The webhook endpoint URL
/// * `payload` - JSON payload to send
/// * `service` - Webhook service type (for service-specific headers)
///
/// # Returns
/// * `Ok(())` if sent successfully (HTTP 2xx status)
/// * `Err` if request failed or returned non-2xx status
async fn send_webhook(webhook_url: &str, payload: &str, service: WebhookService) -> Result<()> {
    // Build headers
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    // Add service-specific headers
    for (header_name, header_value) in service.headers() {
        headers.set(header_name, header_value)?;
    }

    // Build request
    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    init.with_headers(headers);
    init.with_body(Some(payload.into()));

    let request = Request::new_with_init(webhook_url, &init)?;
    let mut response = Fetch::Request(request).send().await?;

    let status_code = response.status_code();
    if !(200..300).contains(&status_code) {
        // Log response body for debugging
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "<unable to read response>".to_string());

        console_error!("Webhook error (HTTP {}): {}", status_code, error_body);

        return Err(Error::RustError(format!(
            "Webhook returned HTTP {}: {}",
            status_code, error_body
        )));
    }

    console_log!("Webhook notification sent successfully");
    Ok(())
}

/// Get current timestamp as ISO string
#[cfg(not(test))]
fn get_timestamp() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_else(|| "unknown".to_string())
}

/// Mock timestamp for tests
#[cfg(test)]
fn get_timestamp() -> String {
    "2025-11-12T10:00:00Z".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("hello"), "hello");
        assert_eq!(escape_json(r#"hello "world""#), r#"hello \"world\""#);
        assert_eq!(escape_json("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_json("tab\there"), "tab\\there");
    }

    #[test]
    fn test_webhook_service_from_url_discord() {
        assert_eq!(
            WebhookService::from_url("https://discord.com/api/webhooks/123/abc"),
            WebhookService::Discord
        );
        assert_eq!(
            WebhookService::from_url("https://discordapp.com/api/webhooks/123/abc"),
            WebhookService::Discord
        );
    }

    #[test]
    fn test_webhook_service_from_url_slack() {
        assert_eq!(
            WebhookService::from_url("https://hooks.slack.com/services/T00/B00/xxx"),
            WebhookService::Slack
        );
    }

    #[test]
    fn test_webhook_service_from_url_zulip() {
        assert_eq!(
            WebhookService::from_url("https://example.zulipchat.com/api/v1/messages"),
            WebhookService::Zulip
        );
        assert_eq!(
            WebhookService::from_url("https://chat.company.com/api/v1/messages"),
            WebhookService::Zulip
        );
    }

    #[test]
    fn test_webhook_service_from_url_generic() {
        assert_eq!(
            WebhookService::from_url("https://example.com/webhook"),
            WebhookService::Generic
        );
    }

    #[test]
    fn test_webhook_service_from_str() {
        use std::str::FromStr;

        assert_eq!(
            WebhookService::from_str("discord"),
            Ok(WebhookService::Discord)
        );
        assert_eq!(
            WebhookService::from_str("Discord"),
            Ok(WebhookService::Discord)
        );
        assert_eq!(
            WebhookService::from_str("DISCORD"),
            Ok(WebhookService::Discord)
        );
        assert_eq!(WebhookService::from_str("slack"), Ok(WebhookService::Slack));
        assert_eq!(WebhookService::from_str("zulip"), Ok(WebhookService::Zulip));
        assert_eq!(
            WebhookService::from_str("generic"),
            Ok(WebhookService::Generic)
        );
        assert_eq!(WebhookService::from_str("unknown"), Err(()));
    }

    #[test]
    fn test_webhook_service_display() {
        assert_eq!(format!("{}", WebhookService::Discord), "Discord");
        assert_eq!(format!("{}", WebhookService::Slack), "Slack");
        assert_eq!(format!("{}", WebhookService::Zulip), "Zulip");
        assert_eq!(format!("{}", WebhookService::Generic), "Generic");
    }

    #[test]
    fn test_build_webhook_payload_discord() {
        let result = CheckResult::failure(
            "https://example.com/test.js".to_string(),
            "Network timeout".to_string(),
        );
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Discord.build_payload(&result, timestamp);

        // Verify Discord-specific format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Network timeout"));
        assert!(payload.contains("embeds"));
        assert!(payload.contains("🔗 Link Check Failed"));
    }

    #[test]
    fn test_build_webhook_payload_slack() {
        let result = CheckResult::failure(
            "https://example.com/test.js".to_string(),
            "SRI mismatch".to_string(),
        );
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Slack.build_payload(&result, timestamp);

        // Verify Slack-specific format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("SRI mismatch"));
        assert!(payload.contains("blocks"));
        assert!(payload.contains("mrkdwn"));
    }

    #[test]
    fn test_build_webhook_payload_zulip() {
        let result = CheckResult::failure(
            "https://example.com/test.js".to_string(),
            "HTTP 404".to_string(),
        );
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Zulip.build_payload(&result, timestamp);

        // Verify Zulip-specific format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("HTTP 404"));
        assert!(payload.contains(r#""type": "stream""#));
        assert!(payload.contains(r#""to": "monitoring""#));
        assert!(payload.contains(r#""topic": "Link Checks""#));
        assert!(payload.contains("## 🔗 Link Check Failed"));
    }

    #[test]
    fn test_build_webhook_payload_generic() {
        let result = CheckResult::failure(
            "https://example.com/test.js".to_string(),
            "Network timeout".to_string(),
        );
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Generic.build_payload(&result, timestamp);

        // Verify generic format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Network timeout"));
        assert!(payload.contains(r#""status": "failure""#));
        assert!(payload.contains(r#""worker": "linkkivahti""#));
    }
}
