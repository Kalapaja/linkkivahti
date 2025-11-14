# Linkkivahti (Link Watcher)

## Project Overview

Linkkivahti is a lightweight Cloudflare Worker-based link availability and SRI (Subresource Integrity) hash checker written in Rust. It periodically verifies that external resources (JavaScript files, CSS, etc.) are accessible and match their expected cryptographic hashes.

**Purpose**: Detect when external dependencies become unavailable or have been tampered with, enabling rapid response to potential security issues or service disruptions.

## Architecture

### Technology Stack

- **Runtime**: Cloudflare Workers (serverless edge computing)
- **Language**: Rust (compiled to WebAssembly)
- **Framework**: `workers-rs` v0.6 (Cloudflare's official Rust SDK)
- **Trigger**: Cron-based scheduling via Workers Cron Triggers
- **Configuration**: TOML with compile-time parsing

### Why Rust + Cloudflare Workers?

1. **Lightweight**: Compiled WASM binary with minimal overhead
2. **Fast execution**: Native performance for hash computation
3. **Zero runtime parsing**: Config embedded at compile time
4. **Cost-effective**: Workers CPU time is billed, so efficiency matters
5. **Edge deployment**: Runs close to monitored resources globally

### High-Level Flow

```
┌─────────────────┐
│  Cron Trigger   │ (e.g., every hour)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Load Config    │ (compile-time embedded TOML)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  For Each URL   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  HTTP GET       │ (fetch resource content)
└────────┬────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
 Success    Error
    │         │
    ▼         │
┌─────────┐  │
│ Compute │  │
│ SRI Hash│  │
└────┬────┘  │
     │       │
 ┌───┴───┐   │
 │       │   │
 ▼       ▼   ▼
Match  Mismatch
 │       │
 OK    FAIL ───────┐
                   │
                   ▼
         ┌──────────────────┐
         │ Log + Send       │
         │ Notification     │
         └──────────────────┘
```

## Configuration System

### Why TOML + Compile-Time Parsing?

**Problem**: Runtime parsing of configuration wastes CPU cycles on every cold start.

**Solution**: Use `static_toml` crate to parse and embed configuration at compile time.

#### Benefits:
- **Zero runtime overhead**: Config is native Rust data structures in binary
- **Compile-time validation**: Invalid config causes build failure, not runtime errors
- **Type safety**: Strongly-typed config access with no deserialization cost
- **Smaller binary**: No need for runtime TOML parser in WASM

### Config Structure

```toml
# config.toml
version = "1.0"

# Global webhook for notifications
webhook_url = "https://discord.com/api/webhooks/..."

[[resources]]
url = "https://cdn.example.com/widget.v1.0.0.js"
sri = "sha384-v5A9WpDBhOK/FsTACnquHK+dgfL9nZO1qHEx00HKn5VsAz1xBp9KNOLuJmPoq1mR"

[[resources]]
url = "https://cdn.example.com/styles.css"
sri = "sha384-abc123..."
```

### Compile-Time Embedding

```rust
use static_toml::static_toml;

static_toml! {
    static CONFIG = include_toml!("config.toml");
}

// Access at runtime with zero parsing cost
fn get_resources() -> &'static [Resource] {
    CONFIG.resources
}
```

## Core Functionality

### 1. Link Availability Checking

**Method**: HTTP GET request

**Why GET instead of HEAD?**
- SRI validation requires the full resource content
- Some CDNs don't properly support HEAD requests
- Marginal performance difference for small resources

**Error Handling**:
- Network failures: DNS errors, connection timeouts, SSL errors
- HTTP errors: 4xx client errors, 5xx server errors
- Timeout: ~30 second limit per request

### 2. SRI Hash Verification

**What is SRI?**

Subresource Integrity (SRI) is a security feature that allows browsers to verify that fetched resources haven't been tampered with.

**Format**: `sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC`

**Supported Algorithms**:
- SHA-256 (sha256)
- SHA-384 (sha384) - **recommended balance of security and performance**
- SHA-512 (sha512)

**Implementation**: Using `ssri` crate for parsing and verification

```rust
use ssri::Integrity;

async fn verify_resource(url: &str, expected_sri: &str) -> Result<bool> {
    // Parse expected integrity
    let integrity: Integrity = expected_sri.parse()?;

    // Fetch resource
    let response = Fetch::Url(url.parse()?).send().await?;
    let content = response.bytes().await?;

    // Verify
    match integrity.check(&content) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}
```

### 3. Notification System

**Dual-Strategy Alerting**:

1. **Console Logging**: Always log all checks for debugging via `wrangler tail`
2. **Webhook Notifications**: Send alerts to external systems on failures

#### Supported Webhook Services

Linkkivahti automatically detects the webhook service type based on the URL and formats payloads accordingly:

**Discord** (`discord.com`, `discordapp.com`)
- Uses Discord webhook embeds format
- Rich formatting with severity-based colors, timestamp, and fields
- Color codes based on error type:
  - SRI mismatch (security issue): Dark red `#992D22` (10038562)
  - Server errors (5xx): Red `#ED4245` (15548997)
  - Client errors (4xx): Orange `#E67E22` (15105570)
  - Network errors: Red-orange (15158332)
- Example payload:
  ```json
  {
    "embeds": [{
      "title": "🔗 Link Check Failed",
      "description": "**https://cdn.example.com/widget.js**",
      "color": 10038562,
      "fields": [
        {"name": "Status", "value": "SRI mismatch (HTTP 200)", "inline": true}
      ],
      "timestamp": "2025-11-12T10:30:00Z"
    }]
  }
  ```

**Slack** (`hooks.slack.com`, `slack.com/api/`)
- Uses Slack Block Kit format
- Includes required `text` field for notification fallback
- Structured sections with mrkdwn formatting and visual dividers
- Example payload:
  ```json
  {
    "text": "Link Check Failed: https://cdn.example.com/widget.js - SRI mismatch (HTTP 200)",
    "blocks": [
      {
        "type": "header",
        "text": {"type": "plain_text", "text": "🔗 Link Check Failed"}
      },
      {"type": "divider"},
      {
        "type": "section",
        "fields": [
          {"type": "mrkdwn", "text": "*URL:*\nhttps://cdn.example.com/widget.js"},
          {"type": "mrkdwn", "text": "*Status:*\nSRI mismatch (HTTP 200)"}
        ]
      },
      {"type": "divider"},
      {
        "type": "context",
        "elements": [
          {"type": "mrkdwn", "text": "Time: 2025-11-12T10:30:00Z | Worker: linkkivahti"}
        ]
      }
    ]
  }
  ```

**Zulip** (`zulipchat.com`, `/external/slack_incoming`)
- Uses Zulip's **Slack-compatible webhook** endpoint
- Accepts standard Slack Block Kit format (same payload as Slack)
- URL format: `https://yourorg.zulipchat.com/api/v1/external/slack_incoming?api_key=YOUR_KEY&stream=STREAM_NAME`
- The `channel` field in Slack payload maps to Zulip stream name
- Example payload: (identical to Slack format above)
  ```json
  {
    "text": "Link Check Failed: https://cdn.example.com/widget.js - SRI mismatch (HTTP 200)",
    "blocks": [
      {
        "type": "header",
        "text": {"type": "plain_text", "text": "🔗 Link Check Failed"}
      },
      {"type": "divider"},
      {
        "type": "section",
        "fields": [
          {"type": "mrkdwn", "text": "*URL:*\nhttps://cdn.example.com/widget.js"},
          {"type": "mrkdwn", "text": "*Status:*\nSRI mismatch (HTTP 200)"}
        ]
      },
      {"type": "divider"},
      {
        "type": "context",
        "elements": [
          {"type": "mrkdwn", "text": "Time: 2025-11-12T10:30:00Z | Worker: linkkivahti"}
        ]
      }
    ]
  }
  ```

**Generic** (fallback for other services)
- Uses **Prometheus Alertmanager v4** webhook format
- Compatible with Grafana, PagerDuty, Opsgenie, VictorOps, and other observability tools
- Includes severity labels (critical for SRI mismatches, warning for other failures)
- Example payload:
  ```json
  {
    "version": "4",
    "groupKey": "linkkivahti/a1b2c3d4e5f6g7h8",
    "truncatedAlerts": 0,
    "status": "firing",
    "receiver": "webhook",
    "groupLabels": {
      "alertname": "LinkCheckFailed"
    },
    "commonLabels": {
      "alertname": "LinkCheckFailed",
      "severity": "critical",
      "service": "linkkivahti"
    },
    "commonAnnotations": {
      "summary": "Link availability check failed",
      "description": "External resource check detected a failure"
    },
    "externalURL": "https://linkkivahti.workers.dev",
    "alerts": [
      {
        "status": "firing",
        "labels": {
          "alertname": "LinkCheckFailed",
          "severity": "critical",
          "service": "linkkivahti",
          "instance": "https://cdn.example.com/widget.js",
          "job": "link-checker"
        },
        "annotations": {
          "summary": "Link check failed for https://cdn.example.com/widget.js",
          "description": "SRI mismatch (HTTP 200)"
        },
        "startsAt": "2025-11-12T10:30:00Z",
        "endsAt": "0001-01-01T00:00:00Z",
        "generatorURL": "https://linkkivahti.workers.dev/",
        "fingerprint": "a1b2c3d4e5f6g7h8"
      }
    ]
  }
  ```

#### Webhook Configuration

**Required Environment Variable**:
```bash
WEBHOOK_URL="https://your-webhook-endpoint"
```

**Optional Override**:
```bash
WEBHOOK_SERVICE="discord|slack|zulip|generic"
```

If `WEBHOOK_SERVICE` is not set, the service type is auto-detected from the URL domain.

**Examples**:

```bash
# Discord (auto-detected)
WEBHOOK_URL="https://discord.com/api/webhooks/123456/abcdef"

# Slack (auto-detected)
WEBHOOK_URL="https://hooks.slack.com/services/T00/B00/xxxx"

# Zulip Slack-compatible webhook (auto-detected)
WEBHOOK_URL="https://yourorg.zulipchat.com/api/v1/external/slack_incoming?api_key=YOUR_KEY&stream=monitoring"

# Alertmanager-compatible endpoint (auto-detected as generic)
WEBHOOK_URL="https://alertmanager.example.com/api/v1/alerts"

# Force specific service for custom domains
WEBHOOK_URL="https://custom.domain/webhook"
WEBHOOK_SERVICE="slack"
```

#### Access Token Configuration

Secured POST routes expect an `Authorization: Bearer <ACCESS_TOKEN>` header. Configure a secret at deploy time:

```bash
wrangler secret put ACCESS_TOKEN
```

If the variable is not provided, Linkkivahti would reject requests to secured endpoints.

#### Implementation Details

The notification system uses Rust's idiomatic patterns:

- **`WebhookService` enum**: Type-safe representation of supported services (Discord, Slack, Zulip, Generic)
- **`impl FromStr`**: Parse service names from environment variables
- **`impl Display`**: Human-readable service names in logs
- **`from_url()` method**: Auto-detection logic based on domain patterns
- **`build_payload()` method**: Service-specific payload formatting
  - Discord: Uses `build_discord_payload()` with severity-based colors
  - Slack/Zulip: Both use `build_slack_payload()` (Zulip is Slack-compatible)
  - Generic: Uses `build_generic_payload()` with Alertmanager v4 format
- **`severity_color()` helper**: Maps error types to Discord color codes
- **`compute_fingerprint()` helper**: Generates stable alert fingerprints for Alertmanager

**Code Reference**: See `src/notify.rs` for the complete implementation.

## Cron Triggers

### Configuration

In `wrangler.toml`:
```toml
[triggers]
crons = ["0 * * * *"]  # Every hour at minute 0
```

**Cron Syntax** (5 fields):
```
┌─────── minute (0 - 59)
│ ┌────── hour (0 - 23)
│ │ ┌───── day of month (1 - 31)
│ │ │ ┌──── month (1 - 12)
│ │ │ │ ┌─── day of week (0 - 6) (Sunday = 0)
│ │ │ │ │
* * * * *
```

**Example Schedules**:
- `0 * * * *` - Every hour
- `*/15 * * * *` - Every 15 minutes
- `0 0 * * *` - Daily at midnight UTC
- `0 9 * * 1-5` - Weekdays at 9 AM

### Implementation

```rust
use worker::*;

#[event(scheduled)]
async fn scheduled(
    event: ScheduledEvent,
    env: Env,
    _ctx: ScheduleContext,
) -> Result<()> {
    console_log!("Cron triggered: {}", event.cron());

    // Run checks for all configured resources
    check_all_resources(&env).await?;

    Ok(())
}
```

## HTTP Interface

### Endpoints

1. **`GET /`**: Combined status and configuration endpoint
   - Returns 200 OK with JSON containing both health status and configuration
   - Shows worker status, version, resource count, and full list of monitored resources
   - Useful for monitoring, debugging, and verification

2. **`POST /check`**: Trigger immediate link check (secured endpoint)
   - Requires `Authorization: Bearer <ACCESS_TOKEN>`
   - Performs the full resource sweep on demand without waiting for the cron schedule

3. **`POST /notify`**: Send webhook self-test (secured endpoint)
   - Requires `Authorization: Bearer <ACCESS_TOKEN>`
   - Issues a synthetic failure payload through the configured webhook to validate alert delivery

4. **Other paths**: 404 Not Found

### Example Response

**Status Endpoint** (at root `/`):
```json
{
  "status": "healthy",
  "worker": "linkkivahti",
  "version": "1.0",
  "resources_count": 2,
  "resources": [
    {
      "url": "https://cdn.example.com/widget.js",
      "sri": "sha384-v5A9W..."
    },
    {
      "url": "https://cdn.example.com/styles.css",
      "sri": "sha384-abc123..."
    }
  ]
}
```

## Testing Strategy

### 1. Unit Tests (Rust)

**What to test**:
- SRI hash computation and verification
- URL validation
- Config parsing (compile-time checks)
- Error message formatting

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sri_verification() {
        let content = b"hello world";
        let expected = compute_sri_sha384(content);
        assert!(verify_sri(content, &expected));
    }
}
```

**Run**: `cargo test`

### 2. Local Integration Testing

**Using wrangler dev**:
```bash
# Start dev server with cron support
wrangler dev --test-scheduled

# Trigger scheduled event
curl "http://localhost:8787/__scheduled?cron=*+*+*+*+*"
```

### 3. Mock Testing

**Approach**: Use dependency injection to mock HTTP client

```rust
#[async_trait]
trait HttpClient {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>>;
}

// Real implementation uses Workers Fetch API
// Test implementation returns mock data
```

## Performance Considerations

### Cold Start Optimization

**Cargo.toml optimizations**:
```toml
[profile.release]
opt-level = "z"        # Optimize for size
lto = true             # Enable Link Time Optimization
codegen-units = 1      # Better optimization, slower compile
strip = true           # Remove debug symbols
panic = "abort"        # Smaller panic handler
```

### Bundle Size

**Target**: < 1 MB WASM binary

**Strategies**:
- Minimal dependencies
- Compile-time config parsing (no runtime parser)
- Avoid large crypto libraries where possible
- Use `cargo-bloat` to identify large dependencies

### Runtime Cost

**Per Invocation**:
- ~0ms: Config access (embedded at compile time)
- ~100-500ms: Network requests (per resource)
- ~1-5ms: Hash computation (SHA-384 on typical file)
- ~10-50ms: Webhook notification

**Estimated Cost**: For 5 resources checked hourly, ~750k CPU ms/month (well within free tier)

## Security Considerations

### 1. SRI Hash Updates

**Challenge**: When legitimate resource updates occur, SRI hash must be updated in config.

**Solution**:
- Monitor for SRI mismatches
- Webhook alerts enable rapid response
- Config update requires code redeploy (intentional friction for security)

### 2. Webhook URL Security

**Risk**: Webhook URL in config could be exposed.

**Mitigation**:
- Store webhook URL as Worker secret (not in code)
- Access via `env.secret("WEBHOOK_URL")?`
- Never log full webhook URL

### 3. Resource Availability Spoofing

**Risk**: Attacker could return valid file at expected URL but with different content.

**Mitigation**: SRI verification prevents this attack - any content change causes hash mismatch.

## Deployment

### Prerequisites

1. Cloudflare account with Workers enabled
2. `wrangler` CLI installed: `npm install -g wrangler`
3. Rust toolchain with `wasm32-unknown-unknown` target
4. `worker-build` tool: `cargo install worker-build`

### Build Process

```bash
# Install target
rustup target add wasm32-unknown-unknown

# Build
worker-build --release

# Output: build/index.js (WASM wrapper + glue code)
```

### Deploy

```bash
# Deploy to production
wrangler deploy

# Deploy to specific environment
wrangler deploy --env staging
```

### Post-Deployment Verification

```bash
# Check worker logs
wrangler tail

# Test status endpoint (shows health + config)
curl https://linkkivahti.yourname.workers.dev/

# Manually trigger cron
curl "https://linkkivahti.yourname.workers.dev/__scheduled?cron=*+*+*+*+*"
```

## Monitoring & Observability

### Logging

**Console logs** are captured and viewable via:
```bash
wrangler tail
wrangler tail --format json
```

**Log Levels**:
- `console_log!()`: Informational messages
- `console_error!()`: Error conditions
- `console_debug!()`: Verbose debugging (only in dev)

### Metrics

**Key Metrics to Track**:
- Check success rate
- Response times per resource
- SRI verification failures
- Webhook delivery success

**Implementation**: Use Workers Analytics Engine or external monitoring service.

### Alerting

**Primary**: Webhook notifications to Discord/Slack

**Backup**: Monitor worker execution errors via Cloudflare dashboard

## Limitations & Constraints

1. **Worker Timeout**: 50ms CPU time (Free), 50-30,000ms (Paid)
   - Solution: Keep checks concurrent, avoid blocking operations

2. **Memory**: 128 MB
   - Solution: Stream large responses, don't load entire files in memory

3. **Bundle Size**: 1 MB compressed (Free), 10 MB (Paid)
   - Solution: Optimize for size, minimal dependencies

4. **Request Size**: 100 MB max
   - Solution: Reasonable for most CDN assets, add size checks

5. **Cron Precision**: ~1 minute accuracy
   - Solution: Accept eventual consistency, not real-time monitoring

## Future Enhancements

1. **Historical Tracking**: Store check results in Workers KV or D1
2. **Trend Analysis**: Track response time trends over time
3. **Multi-Region Checks**: Verify from multiple edge locations
4. **Auto-Update SRI**: Detect legitimate updates and prompt for config update
5. **Custom Retry Logic**: Exponential backoff for transient failures
6. **Rate Limiting**: Throttle checks to respect CDN rate limits

## References

- [Cloudflare Workers Docs](https://developers.cloudflare.com/workers/)
- [workers-rs GitHub](https://github.com/cloudflare/workers-rs)
- [Cron Triggers](https://developers.cloudflare.com/workers/configuration/cron-triggers/)
- [SRI Specification](https://www.w3.org/TR/SRI/)
- [static_toml Crate](https://github.com/cptpiepmatz/static-toml)
- [ssri Crate](https://docs.rs/ssri/)

## License

Linkkivahti (Link Watcher) is distributed under the GNU General Public License, version 3 or (at your option) any later version. Copyright (C) 2025 Kalapaja Oy.
