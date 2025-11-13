# Linkkivahti (Link Watcher)

A lightweight Cloudflare Worker written in Rust that periodically monitors external resources for availability and validates their Subresource Integrity (SRI) hashes.

## Features

- **Compile-time configuration**: TOML config parsed at build time for zero runtime overhead
- **SRI validation**: Cryptographic verification of resource integrity
- **Cron-based scheduling**: Automated periodic checks via Cloudflare Workers Cron Triggers
- **Webhook notifications**: Alerts on failures via Discord, Slack, or any webhook endpoint
- **HTTP endpoints**: Health check and config inspection APIs
- **Minimal overhead**: Written in Rust, compiled to WebAssembly

## Prerequisites

- [Rust](https://rustup.rs/) toolchain
- [Wrangler CLI](https://developers.cloudflare.com/workers/wrangler/install-and-update/): `npm install -g wrangler`
- Cloudflare account with Workers enabled
- `worker-build` tool (installed automatically during build)

## Quick Start

### 1. Install Dependencies

```bash
# Install Rust WASM target
rustup target add wasm32-unknown-unknown

# Login to Cloudflare
wrangler login
```

### 2. Configure Resources

Edit `config.toml` to add resources to monitor:

```toml
version = "1.0"

[[resources]]
url = "https://cdn.example.com/script.js"
sri = "sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC"

[[resources]]
url = "https://cdn.example.com/style.css"
sri = "sha384-..."
```

**Generating SRI Hashes:**

```bash
# Using openssl
curl -s https://example.com/file.js | openssl dgst -sha384 -binary | openssl base64 -A
# Then prepend "sha384-" to the result

# Using Node.js
node -e "const c=require('crypto'),h=c.createHash('sha384');process.stdin.on('data',d=>h.update(d));process.stdin.on('end',()=>console.log('sha384-'+h.digest('base64')))" < file.js

# Or check existing SRI in browser DevTools
# Look for <script integrity="sha384-..."> in page source
```

### 3. Configure Webhook (Optional)

Set up webhook URL as a secret (keeps auth tokens private):

```bash
wrangler secret put WEBHOOK_URL
```

Paste your webhook URL when prompted. Linkkivahti **automatically detects** the webhook service based on the URL and formats notifications accordingly.

**Supported services:**

- **Discord**: `https://discord.com/api/webhooks/YOUR_WEBHOOK_ID/YOUR_TOKEN`
  - Auto-detected from domain, uses rich embeds format
  
- **Slack**: `https://hooks.slack.com/services/YOUR/WEBHOOK/URL`
  - Auto-detected from domain, uses Block Kit format
  
- **Zulip**: `https://yourorg.zulipchat.com/api/v1/messages?api_key=YOUR_API_KEY`
  - Auto-detected from domain or `/api/v1/messages` path
  - Sends to "monitoring" stream with "Link Checks" topic
  
- **Generic**: Any other endpoint accepting JSON POST
  - Simple JSON format for custom integrations

**Manual override** (optional):

If your webhook service uses a custom domain, you can force a specific format:

```bash
# Set the webhook URL
wrangler secret put WEBHOOK_URL

# Set the service type override
wrangler secret put WEBHOOK_SERVICE
# Enter one of: discord, slack, zulip, generic
```

### 4. Configure Cron Schedule

Edit `wrangler.toml` to adjust check frequency:

```toml
[triggers]
crons = ["0 * * * *"]  # Every hour (default)
```

**Example schedules:**

- `*/15 * * * *` - Every 15 minutes
- `0 0 * * *` - Daily at midnight UTC
- `0 9 * * 1-5` - Weekdays at 9 AM UTC

### 5. Deploy

```bash
# Build and deploy
wrangler deploy

# Or deploy to a specific environment
wrangler deploy --env production
```

## Usage

### Monitoring Logs

View real-time logs:

```bash
wrangler tail
```

### HTTP Endpoints

Once deployed, your worker exposes:

- **`GET /`**: Combined status and configuration endpoint

Example:

```bash
curl https://linkkivahti.yourname.workers.dev/
```

Response:

```json
{
  "status": "healthy",
  "worker": "linkkivahti",
  "version": "1.0",
  "resources": [
    {
      "url": "https://cdn.example.com/script.js",
      "sri": "sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC"
    },
    {
      "url": "https://cdn.example.com/style.css",
      "sri": "sha384-..."
    }
  ]
}
```

### Manual Trigger (Development)

Trigger a check manually during development:

```bash
# Start dev server with cron support
wrangler dev --test-scheduled

# In another terminal, trigger scheduled event
curl "http://localhost:8787/__scheduled?cron=*+*+*+*+*"
```

## Configuration Reference

### config.toml

```toml
version = "1.0"

[[resources]]
url = "URL to check"
sri = "Expected SRI hash (sha256-... or sha384-... or sha512-...)"

# Add more resources as needed
[[resources]]
url = "..."
sri = "..."
```

**Fields:**

- `version`: Config version (informational)
- `resources`: Array of resources to monitor
  - `url`: Full URL of the resource
  - `sri`: Expected SRI hash in format `sha384-BASE64HASH`

### wrangler.toml

```toml
name = "linkkivahti"
main = "build/index.js"
compatibility_date = "2025-11-12"

[build]
command = "cargo install -q worker-build && worker-build --release"

[triggers]
crons = ["0 * * * *"]  # Adjust schedule here
```

### Secrets

Set via `wrangler secret put`:

- `WEBHOOK_URL`: Webhook endpoint for failure notifications (optional)
  - Supports Discord, Slack, Zulip, and generic webhooks
  - Service type auto-detected from URL
  
- `WEBHOOK_SERVICE`: Override auto-detection (optional)
  - Values: `discord`, `slack`, `zulip`, `generic`
  - Only needed for custom domains that don't match standard patterns

## Webhook Notification Formats

Linkkivahti automatically formats notifications based on the detected webhook service.

### Discord Format

Rich embeds with color coding:

```json
{
  "embeds": [
    {
      "title": "🔗 Link Check Failed",
      "description": "**https://example.com/file.js**",
      "color": 15158332,
      "fields": [
        {"name": "Status", "value": "SRI mismatch", "inline": true},
        {"name": "Time", "value": "2025-11-12T10:30:00Z", "inline": true}
      ]
    }
  ]
}
```

### Slack Format

Block Kit with structured sections:

```json
{
  "blocks": [
    {
      "type": "header",
      "text": {"type": "plain_text", "text": "🔗 Link Check Failed"}
    },
    {
      "type": "section",
      "fields": [
        {"type": "mrkdwn", "text": "*URL:*\nhttps://example.com/file.js"},
        {"type": "mrkdwn", "text": "*Status:*\nSRI mismatch"}
      ]
    },
    {
      "type": "context",
      "elements": [
        {"type": "mrkdwn", "text": "Time: 2025-11-12T10:30:00Z | Worker: linkkivahti"}
      ]
    }
  ]
}
```

### Zulip Format

Stream message with markdown:

```json
{
  "type": "stream",
  "to": "monitoring",
  "topic": "Link Checks",
  "content": "## 🔗 Link Check Failed\n\n**URL:** https://example.com/file.js\n\n**Status:** SRI mismatch\n\n**Time:** 2025-11-12T10:30:00Z"
}
```

### Generic Format

Simple JSON for custom integrations:

```json
{
  "timestamp": "2025-11-12T10:30:00Z",
  "status": "failure",
  "url": "https://example.com/file.js",
  "error": "SRI mismatch",
  "worker": "linkkivahti"
}
```

## Development

### Project Structure

```
linkkivahti/
├── src/
│   ├── lib.rs         # Main worker entry point
│   ├── config.rs      # Compile-time config parsing
│   ├── checker.rs     # Link checking and SRI verification
│   └── notify.rs      # Webhook notifications
├── config.toml        # Resource configuration
├── wrangler.toml      # Worker configuration
├── Cargo.toml         # Rust dependencies
├── CLAUDE.md          # Detailed architecture documentation
└── README.md          # This file
```

### Running Tests

```bash
cargo test
```

### Local Development

```bash
# Start local dev server
wrangler dev

# Access status endpoint
curl http://localhost:8787/
```

### Build Optimization

The project uses aggressive size optimization:

```toml
[profile.release]
opt-level = "z"        # Optimize for size
lto = true             # Link Time Optimization
codegen-units = 1      # Better optimization
strip = true           # Remove debug symbols
panic = "abort"        # Smaller panic handler
```

Expected binary size: < 500 KB compressed

## Troubleshooting

### Build fails with "No such file or directory (os error 2)"

Ensure `config.toml` exists in the project root.

### "Invalid SRI format" errors

Check that SRI hashes start with `sha256-`, `sha384-`, or `sha512-` and use valid Base64 encoding.

### Webhook notifications not working

1. Verify `WEBHOOK_URL` secret is set: `wrangler secret list`
2. Check webhook URL is valid and accessible
3. Review logs: `wrangler tail`

### Cron not triggering

1. Verify `[triggers]` section in `wrangler.toml`
2. Check Cloudflare dashboard for cron status
3. Cron may take a few minutes to activate after deployment

## Performance

**Cold start:** ~10-50ms (WASM initialization)  
**Check time:** ~100-500ms per resource (network dependent)  
**Memory:** < 10 MB typical usage  
**CPU:** ~1-5ms for SRI computation  

**Cost estimate:** For 5 resources checked hourly, ~750k CPU ms/month (within free tier).

## Security

- SRI hashes verified using cryptographic checksums
- Webhook URLs stored as secrets (not in code)
- No sensitive data logged
- Config changes require redeployment (intentional security friction)

## Contributing

Contributions welcome! Please ensure:

1. Code passes `cargo check` and `cargo test`
2. New features documented in README and CLAUDE.md
3. Follows existing code style

## License

MIT OR Apache-2.0

## References

- [CLAUDE.md](./CLAUDE.md) - Detailed technical documentation
- [Cloudflare Workers](https://developers.cloudflare.com/workers/)
- [workers-rs](https://github.com/cloudflare/workers-rs)
- [SRI Specification](https://www.w3.org/TR/SRI/)
