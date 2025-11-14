//! Notification module for sending alerts about check failures

use crate::checker::CheckResult;
use serde::Serialize;
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

// Discord webhook payload structures
#[derive(Serialize)]
struct DiscordPayload {
    embeds: Vec<DiscordEmbed>,
}

#[derive(Serialize)]
struct DiscordEmbed {
    title: &'static str,
    description: String,
    color: u32,
    fields: Vec<DiscordField>,
    timestamp: String,
}

#[derive(Serialize)]
struct DiscordField {
    name: &'static str,
    value: String,
    inline: bool,
}

// Slack webhook payload structures
#[derive(Serialize)]
struct SlackPayload {
    text: String,
    blocks: Vec<SlackBlock>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum SlackBlock {
    #[serde(rename = "header")]
    Header { text: SlackText },
    #[serde(rename = "section")]
    Section { fields: Vec<SlackText> },
    #[serde(rename = "context")]
    Context { elements: Vec<SlackText> },
    #[serde(rename = "divider")]
    Divider,
}

#[derive(Serialize)]
struct SlackText {
    #[serde(rename = "type")]
    text_type: &'static str,
    text: String,
}

// Alertmanager v4 webhook payload structures (for generic/observability tools)
#[derive(Serialize)]
struct AlertmanagerPayload {
    version: &'static str,
    #[serde(rename = "groupKey")]
    group_key: String,
    #[serde(rename = "truncatedAlerts")]
    truncated_alerts: u32,
    status: &'static str,
    receiver: &'static str,
    #[serde(rename = "groupLabels")]
    group_labels: AlertmanagerLabels,
    #[serde(rename = "commonLabels")]
    common_labels: AlertmanagerLabels,
    #[serde(rename = "commonAnnotations")]
    common_annotations: AlertmanagerAnnotations,
    #[serde(rename = "externalURL")]
    external_url: &'static str,
    alerts: Vec<AlertmanagerAlert>,
}

#[derive(Serialize)]
struct AlertmanagerLabels {
    alertname: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    service: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    job: Option<&'static str>,
}

#[derive(Serialize)]
struct AlertmanagerAnnotations {
    summary: String,
    description: String,
}

#[derive(Serialize)]
struct AlertmanagerAlert {
    status: &'static str,
    labels: AlertmanagerLabels,
    annotations: AlertmanagerAnnotations,
    #[serde(rename = "startsAt")]
    starts_at: String,
    #[serde(rename = "endsAt")]
    ends_at: &'static str,
    #[serde(rename = "generatorURL")]
    generator_url: &'static str,
    fingerprint: String,
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
        // Helper for case-insensitive substring search without allocation
        fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
            if needle.is_empty() {
                return true;
            }
            haystack.len() >= needle.len()
                && haystack
                    .as_bytes()
                    .windows(needle.len())
                    .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
        }

        // Check for Discord domains
        if contains_ignore_ascii_case(url, "discord.com")
            || contains_ignore_ascii_case(url, "discordapp.com")
        {
            Self::Discord
        // Check for Slack domains
        } else if contains_ignore_ascii_case(url, "hooks.slack.com")
            || contains_ignore_ascii_case(url, "slack.com/api/")
        {
            Self::Slack
        // Check for Zulip Slack-compatible webhook (uses Slack format)
        } else if contains_ignore_ascii_case(url, "zulipchat.com")
            || url.contains("/external/slack_incoming")
        {
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
    fn build_payload(&self, result: &CheckResult, timestamp: &str) -> Result<String> {
        let json = match self {
            Self::Discord => Self::build_discord_payload(result, timestamp)?,
            Self::Slack | Self::Zulip => Self::build_slack_payload(result, timestamp)?,
            Self::Generic => Self::build_generic_payload(result, timestamp)?,
        };
        Ok(json)
    }

    /// Build Discord webhook payload with embeds
    fn build_discord_payload(result: &CheckResult, timestamp: &str) -> Result<String> {
        let color = Self::severity_color(result);

        let payload = DiscordPayload {
            embeds: vec![DiscordEmbed {
                title: "🔗 Link Check Failed",
                description: format!("**{}**", result.url),
                color,
                fields: vec![DiscordField {
                    name: "Status",
                    value: result.description().to_string(),
                    inline: true,
                }],
                timestamp: timestamp.to_string(),
            }],
        };

        serde_json::to_string(&payload)
            .map_err(|e| Error::RustError(format!("Failed to serialize Discord payload: {}", e)))
    }

    /// Get Discord color code based on error severity
    fn severity_color(result: &CheckResult) -> u32 {
        use crate::checker::CheckError;

        // SRI mismatch is a security issue - dark red
        if result.sri_valid == Some(false) {
            return 10038562; // Dark red #992D22
        }

        // Color based on error type
        match result.error {
            Some(CheckError::HttpError(code)) if code >= 500 => 15548997, // Server error - red #ED4245
            Some(CheckError::HttpError(_)) => 15105570, // Client error - orange #E67E22
            Some(CheckError::FetchFailed) => 15158332,  // Network error - red-orange
            _ => 15548997,                              // Default - red #ED4245
        }
    }

    /// Build Slack webhook payload with Block Kit
    fn build_slack_payload(result: &CheckResult, timestamp: &str) -> Result<String> {
        let fallback_text = format!(
            "Link Check Failed: {} - {}",
            result.url,
            result.description()
        );

        let payload = SlackPayload {
            text: fallback_text,
            blocks: vec![
                SlackBlock::Header {
                    text: SlackText {
                        text_type: "plain_text",
                        text: "🔗 Link Check Failed".to_string(),
                    },
                },
                SlackBlock::Divider,
                SlackBlock::Section {
                    fields: vec![
                        SlackText {
                            text_type: "mrkdwn",
                            text: format!("*URL:*\n{}", result.url),
                        },
                        SlackText {
                            text_type: "mrkdwn",
                            text: format!("*Status:*\n{}", result.description()),
                        },
                    ],
                },
                SlackBlock::Divider,
                SlackBlock::Context {
                    elements: vec![SlackText {
                        text_type: "mrkdwn",
                        text: format!("Time: {} | Worker: linkkivahti", timestamp),
                    }],
                },
            ],
        };

        serde_json::to_string(&payload)
            .map_err(|e| Error::RustError(format!("Failed to serialize Slack payload: {}", e)))
    }

    /// Build Alertmanager v4 webhook payload for observability tools
    fn build_generic_payload(result: &CheckResult, timestamp: &str) -> Result<String> {
        let severity = if result.sri_valid == Some(false) {
            "critical" // SRI mismatch is a security issue
        } else {
            "warning" // Other failures are warnings
        };

        let summary = format!("Link check failed for {}", result.url);
        let description = result.description();
        let fingerprint = Self::compute_fingerprint(result.url);
        let group_key = format!("linkkivahti/{}", fingerprint);

        let payload = AlertmanagerPayload {
            version: "4",
            group_key,
            truncated_alerts: 0,
            status: "firing",
            receiver: "webhook",
            group_labels: AlertmanagerLabels {
                alertname: "LinkCheckFailed",
                severity: None,
                service: None,
                instance: None,
                job: None,
            },
            common_labels: AlertmanagerLabels {
                alertname: "LinkCheckFailed",
                severity: Some(severity),
                service: Some("linkkivahti"),
                instance: None,
                job: None,
            },
            common_annotations: AlertmanagerAnnotations {
                summary: "Link availability check failed".to_string(),
                description: "External resource check detected a failure".to_string(),
            },
            external_url: "https://linkkivahti.workers.dev",
            alerts: vec![AlertmanagerAlert {
                status: "firing",
                labels: AlertmanagerLabels {
                    alertname: "LinkCheckFailed",
                    severity: Some(severity),
                    service: Some("linkkivahti"),
                    instance: Some(result.url.to_string()),
                    job: Some("link-checker"),
                },
                annotations: AlertmanagerAnnotations {
                    summary,
                    description,
                },
                starts_at: timestamp.to_string(),
                ends_at: "0001-01-01T00:00:00Z", // Zero value indicates ongoing
                generator_url: "https://linkkivahti.workers.dev/",
                fingerprint,
            }],
        };

        serde_json::to_string(&payload).map_err(|e| {
            Error::RustError(format!("Failed to serialize Alertmanager payload: {}", e))
        })
    }

    /// Compute a fingerprint hash for an alert based on the URL
    fn compute_fingerprint(url: &str) -> String {
        // Simple hash computation - use first 16 chars of hex representation
        let mut hash: u64 = 0;
        for byte in url.as_bytes() {
            hash = hash.wrapping_mul(31).wrapping_add(*byte as u64);
        }
        format!("{:016x}", hash)
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
    let payload = service.build_payload(result, &timestamp)?;

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

/// Send a webhook notification via HTTP POST
///
/// Sends a formatted payload to the webhook endpoint. Logs detailed error
/// information if the request fails.
///
/// # Arguments
/// * `webhook_url` - The webhook endpoint URL
/// * `payload` - JSON payload to send
/// * `service` - Webhook service type (for logging)
///
/// # Returns
/// * `Ok(())` if sent successfully (HTTP 2xx status)
/// * `Err` if request failed or returned non-2xx status
async fn send_webhook(webhook_url: &str, payload: &str, _service: WebhookService) -> Result<()> {
    // Build headers
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

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
            WebhookService::from_url(
                "https://example.zulipchat.com/api/v1/external/slack_incoming"
            ),
            WebhookService::Zulip
        );
        assert_eq!(
            WebhookService::from_url("https://chat.company.com/api/v1/external/slack_incoming"),
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
        use crate::checker::CheckError;

        let result = CheckResult::failure("https://example.com/test.js", CheckError::FetchFailed);
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Discord
            .build_payload(&result, timestamp)
            .unwrap();

        // Verify Discord-specific format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Fetch failed"));
        assert!(payload.contains("embeds"));
        assert!(payload.contains("🔗 Link Check Failed"));
        assert!(payload.contains("timestamp")); // New: timestamp field
        assert!(payload.contains("2025-11-12T10:00:00Z"));
        // Color should be 15158332 for network errors
        assert!(payload.contains("15158332"));
    }

    #[test]
    fn test_severity_color() {
        use crate::checker::CheckError;

        // SRI mismatch should be dark red
        let sri_fail = CheckResult::success("https://example.com/test.js", 200, false);
        let color = WebhookService::severity_color(&sri_fail);
        assert_eq!(color, 10038562);

        // Server error should be red
        let server_error =
            CheckResult::failure("https://example.com/test.js", CheckError::HttpError(500));
        let color = WebhookService::severity_color(&server_error);
        assert_eq!(color, 15548997);

        // Client error should be orange
        let client_error =
            CheckResult::failure("https://example.com/test.js", CheckError::HttpError(404));
        let color = WebhookService::severity_color(&client_error);
        assert_eq!(color, 15105570);

        // Network error should be red-orange
        let network_error =
            CheckResult::failure("https://example.com/test.js", CheckError::FetchFailed);
        let color = WebhookService::severity_color(&network_error);
        assert_eq!(color, 15158332);
    }

    #[test]
    fn test_build_webhook_payload_slack() {
        use crate::checker::CheckError;

        let result =
            CheckResult::failure("https://example.com/test.js", CheckError::BodyReadFailed);
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Slack
            .build_payload(&result, timestamp)
            .unwrap();

        // Verify Slack-specific format
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Failed to read response body"));
        assert!(payload.contains("blocks"));
        assert!(payload.contains("mrkdwn"));
        // New: required text field for fallback
        assert!(payload.contains(r#""text":"Link Check Failed:"#));
        // New: divider blocks
        assert!(payload.contains(r#""type":"divider""#));
    }

    #[test]
    fn test_build_webhook_payload_zulip() {
        use crate::checker::CheckError;

        let result =
            CheckResult::failure("https://example.com/test.js", CheckError::HttpError(404));
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Zulip
            .build_payload(&result, timestamp)
            .unwrap();

        // Verify Zulip uses Slack format (Slack-compatible webhook)
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("HTTP error: 404"));
        assert!(payload.contains("blocks"));
        assert!(payload.contains("mrkdwn"));
        assert!(payload.contains(r#""text":"Link Check Failed:"#));
        assert!(payload.contains(r#""type":"divider""#));
    }

    #[test]
    fn test_build_webhook_payload_generic() {
        use crate::checker::CheckError;

        let result = CheckResult::failure("https://example.com/test.js", CheckError::FetchFailed);
        let timestamp = "2025-11-12T10:00:00Z";

        let payload = WebhookService::Generic
            .build_payload(&result, timestamp)
            .unwrap();

        // Verify Alertmanager v4 format
        assert!(payload.contains(r#""version":"4""#));
        assert!(payload.contains(r#""status":"firing""#));
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Fetch failed"));
        assert!(payload.contains(r#""alertname":"LinkCheckFailed""#));
        assert!(payload.contains(r#""severity":"warning""#)); // Network errors are warnings
        assert!(payload.contains(r#""service":"linkkivahti""#));
        assert!(payload.contains(r#""job":"link-checker""#));
        assert!(payload.contains("alerts"));
        assert!(payload.contains("fingerprint"));
        assert!(payload.contains(r#""startsAt":"2025-11-12T10:00:00Z""#));
    }

    #[test]
    fn test_alertmanager_severity() {
        use crate::checker::CheckError;

        // SRI mismatch should be critical
        let sri_fail = CheckResult::success("https://example.com/test.js", 200, false);
        let payload = WebhookService::Generic
            .build_payload(&sri_fail, "2025-11-12T10:00:00Z")
            .unwrap();
        assert!(payload.contains(r#""severity":"critical""#));

        // Other errors should be warning
        let network_error =
            CheckResult::failure("https://example.com/test.js", CheckError::FetchFailed);
        let payload = WebhookService::Generic
            .build_payload(&network_error, "2025-11-12T10:00:00Z")
            .unwrap();
        assert!(payload.contains(r#""severity":"warning""#));
    }

    #[test]
    fn test_compute_fingerprint() {
        // Same URL should produce same fingerprint
        let fp1 = WebhookService::compute_fingerprint("https://example.com/test.js");
        let fp2 = WebhookService::compute_fingerprint("https://example.com/test.js");
        assert_eq!(fp1, fp2);

        // Different URLs should produce different fingerprints
        let fp3 = WebhookService::compute_fingerprint("https://example.com/other.js");
        assert_ne!(fp1, fp3);

        // Fingerprint should be 16 hex chars
        assert_eq!(fp1.len(), 16);
        assert!(fp1.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
