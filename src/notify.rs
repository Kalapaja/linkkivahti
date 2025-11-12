//! Notification module for sending alerts about check failures

use crate::checker::CheckResult;
use worker::*;

/// Send a notification about a failed check to the configured webhook
///
/// # Arguments
/// * `env` - Worker environment to access WEBHOOK_URL secret
/// * `result` - The check result to report
///
/// # Returns
/// Ok(()) if notification was sent successfully, Err otherwise
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

    console_log!("Sending webhook notification for: {}", result.url);

    // Build notification payload
    let payload = build_webhook_payload(result);

    // Send webhook
    match send_webhook(&webhook_url, &payload).await {
        Ok(_) => {
            console_log!("Notification sent successfully");
            Ok(())
        }
        Err(e) => {
            console_error!("Failed to send notification: {}", e);
            Err(e)
        }
    }
}

/// Build a JSON payload for the webhook notification
///
/// The format is designed to work with Discord/Slack webhooks as well as
/// generic JSON endpoints.
fn build_webhook_payload(result: &CheckResult) -> String {
    // Get current timestamp (only works in WASM/Worker environment)
    let timestamp = get_timestamp();

    // Try to detect Discord webhook
    let is_discord = true; // We'll build Discord-compatible format by default

    if is_discord {
        // Discord webhook format
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
    } else {
        // Generic JSON format
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
async fn send_webhook(webhook_url: &str, payload: &str) -> Result<()> {
    let headers = Headers::new();
    headers.set("Content-Type", "application/json")?;

    let mut init = RequestInit::new();
    init.with_method(Method::Post);
    init.with_headers(headers);
    init.with_body(Some(payload.into()));

    let request = Request::new_with_init(webhook_url, &init)?;
    let response = Fetch::Request(request).send().await?;

    let status_code = response.status_code();
    if !(200..300).contains(&status_code) {
        return Err(Error::RustError(format!(
            "Webhook returned HTTP {}",
            status_code
        )));
    }

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
    fn test_build_webhook_payload() {
        let result = CheckResult::failure(
            "https://example.com/test.js".to_string(),
            "Network timeout".to_string(),
        );

        let payload = build_webhook_payload(&result);

        // Verify it's valid-looking JSON
        assert!(payload.contains("https://example.com/test.js"));
        assert!(payload.contains("Network timeout"));
        assert!(payload.contains("embeds")); // Discord format
    }
}
