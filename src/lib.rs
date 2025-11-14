//! Linkkivahti - Link availability and SRI hash checker for Cloudflare Workers
//!
//! This worker periodically checks configured URLs for availability and verifies
//! their Subresource Integrity (SRI) hashes, alerting on failures via webhooks.

mod checker;
mod config;
mod notify;
pub mod sri;

use checker::check_resource;
use futures::future::join_all;
use serde::Serialize;
use worker::*;

/// Status response structure for the / endpoint
#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    worker: &'static str,
    version: &'static str,
    resources: Vec<ResourceInfo>,
}

/// Individual resource information for status endpoint
#[derive(Serialize)]
struct ResourceInfo {
    url: &'static str,
    sri: &'static str,
}

pub async fn check_all_resources(env: &Env) {
    console_log!(
        "🔍 Starting link checks for {} resources",
        config::resource_count()
    );

    // Check all resources in parallel
    let check_futures: Vec<_> = config::resources()
        .iter()
        .map(|resource| check_resource(&resource.url, &resource.sri))
        .collect();

    let results = join_all(check_futures).await;

    // Send notifications for any problems
    for result in &results {
        if result.has_problem() {
            console_error!(
                "Problem detected: {} - {}",
                result.url,
                result.description()
            );
            if let Err(e) = notify::send_failure_notification(env, result).await {
                console_error!("Failed to send notification: {}", e);
            }
        }
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

/// Scheduled event handler - triggered by cron
///
/// This checks all configured resources and sends notifications for any failures.
#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    check_all_resources(&env).await;
}

/// Check Authorization header for secured endpoints to match the access token
/// (as configured by the ACCESS_TOKEN environment variable, with a random built-in fallback).
/// Returns an error if the token is missing or invalid.
/// # Arguments
/// * `req` - The incoming HTTP request
/// # Returns
/// Ok(()) if authorized, Err otherwise
fn check_auth(env: &Env, req: &Request) -> Result<()> {
    if let Ok(access_token) = env.secret("ACCESS_TOKEN") {
        let auth_header = req
            .headers()
            .get("Authorization")?
            .ok_or_else(|| Error::RustError("Missing Authorization header".to_string()))?;

        if auth_header != format!("Bearer {}", access_token) {
            return Err(Error::RustError("Unauthorized".to_string()));
        }

        Ok(())
    } else {
        Err(Error::RustError("ACCESS_TOKEN not configured".to_string()))
    }
}

/// HTTP fetch event handler
///
/// Provides:
/// - GET / - Combined health and configuration endpoint
/// - POST /check - Trigger immediate link check (secured with access token)
/// - POST /notify - Test notification webhook (secured with access token)
/// - Other paths return 404
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let url = req.url()?;
    let path = url.path();

    match (req.method(), path.as_ref()) {
        (Method::Get, "/") => handle_status(),
        (Method::Post, "/check") => {
            check_auth(&env, &req)?;
            check_all_resources(&env).await;
            Response::from_html("Link check triggered")
        }
        (Method::Post, "/notify") => {
            check_auth(&env, &req)?;
            notify::send_test_notification(&env).await?;
            Response::from_html("Test notification sent")
        }
        _ => Response::error("Not Found", 404),
    }
}

/// Handle / (root) endpoint
///
/// Returns combined health status and configuration in a single response
fn handle_status() -> Result<Response> {
    let resources: Vec<ResourceInfo> = config::resources()
        .iter()
        .map(|r| ResourceInfo {
            url: r.url,
            sri: r.sri,
        })
        .collect();

    let status = StatusResponse {
        status: "healthy",
        worker: "linkkivahti",
        version: config::version(),
        resources,
    };

    Response::from_json(&status)
}

#[cfg(test)]
mod tests {
    // No tests needed for this module currently
    // Integration tests would require Workers runtime
}
