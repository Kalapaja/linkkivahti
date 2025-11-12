//! Linkkivahti - Link availability and SRI hash checker for Cloudflare Workers
//!
//! This worker periodically checks configured URLs for availability and verifies
//! their Subresource Integrity (SRI) hashes, alerting on failures via webhooks.

mod checker;
mod config;
mod notify;

use checker::check_resource;
use worker::*;

/// Scheduled event handler - triggered by cron
///
/// This checks all configured resources and sends notifications for any failures.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    console_log!(
        "🔍 Starting link checks for {} resources",
        config::resource_count()
    );

    let mut results = Vec::new();

    // Check all configured resources
    for resource in config::resources() {
        let result = check_resource(&resource.url, &resource.sri).await;

        // Send notification if there's a problem
        if result.has_problem() {
            console_error!("Problem detected: {} - {}", result.url, result.description());
            if let Err(e) = notify::send_failure_notification(&env, &result).await {
                console_error!("Failed to send notification: {}", e);
            }
        }

        results.push(result);
    }

    // Log summary
    let successful = results.iter().filter(|r| !r.has_problem()).count();
    let failed = results.len() - successful;

    console_log!(
        "✓ Check complete: {}/{} successful, {} failed",
        successful,
        results.len(),
        failed
    );
}

/// HTTP fetch event handler
///
/// Provides:
/// - GET /health - Health check endpoint
/// - GET /config - Configuration dump
/// - Other paths return 404
#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path();

    match (req.method(), path.as_ref()) {
        (Method::Get, "/health") => handle_health(),
        (Method::Get, "/config") => handle_config(),
        _ => Response::error("Not Found", 404),
    }
}

/// Handle /health endpoint
///
/// Returns a simple health check response with worker status
fn handle_health() -> Result<Response> {
    let health_json = format!(
        r#"{{
  "status": "healthy",
  "worker": "linkkivahti",
  "version": "{}",
  "resources_count": {}
}}"#,
        config::version(),
        config::resource_count()
    );

    let mut response = Response::ok(health_json)?;
    let headers = response.headers_mut();
    headers.set("Content-Type", "application/json")?;
    Ok(response)
}

/// Handle /config endpoint
///
/// Returns the compiled-in configuration (resources to monitor)
fn handle_config() -> Result<Response> {
    let mut config_json = format!(
        r#"{{
  "version": "{}",
  "resources": ["#,
        config::version()
    );

    for (i, resource) in config::resources().iter().enumerate() {
        if i > 0 {
            config_json.push_str(",");
        }
        config_json.push_str(&format!(
            r#"
    {{"url": "{}", "sri": "{}"}}"#,
            escape_json(&resource.url),
            escape_json(&resource.sri)
        ));
    }

    config_json.push_str(
        r#"
  ]
}"#,
    );

    let mut response = Response::ok(config_json)?;
    let headers = response.headers_mut();
    headers.set("Content-Type", "application/json")?;
    Ok(response)
}

/// Escape a string for safe inclusion in JSON
fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("hello"), "hello");
        assert_eq!(escape_json(r#"test "quotes""#), r#"test \"quotes\""#);
    }
}