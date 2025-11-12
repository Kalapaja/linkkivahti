//! Link availability and SRI verification module

use ssri::Integrity;
use worker::*;

/// Result of a link check operation
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub url: String,
    pub success: bool,
    pub status_code: Option<u16>,
    pub error_message: Option<String>,
    pub sri_valid: Option<bool>,
}

impl CheckResult {
    /// Create a successful check result
    pub fn success(url: String, status_code: u16, sri_valid: bool) -> Self {
        Self {
            url,
            success: true,
            status_code: Some(status_code),
            error_message: None,
            sri_valid: Some(sri_valid),
        }
    }

    /// Create a failed check result
    pub fn failure(url: String, error: String) -> Self {
        Self {
            url,
            success: false,
            status_code: None,
            error_message: Some(error),
            sri_valid: None,
        }
    }

    /// Check if this result indicates a problem (failure or SRI mismatch)
    pub fn has_problem(&self) -> bool {
        !self.success || self.sri_valid == Some(false)
    }

    /// Get a human-readable description of the result
    pub fn description(&self) -> String {
        if !self.success {
            format!("Failed: {}", self.error_message.as_ref().unwrap())
        } else if self.sri_valid == Some(false) {
            format!(
                "SRI mismatch (HTTP {})",
                self.status_code.unwrap_or(0)
            )
        } else {
            format!("OK (HTTP {})", self.status_code.unwrap_or(0))
        }
    }
}

/// Check a single resource: verify it's accessible and SRI hash matches
///
/// This performs:
/// 1. HTTP GET request to fetch the resource content
/// 2. SRI hash verification against expected hash
///
/// # Arguments
/// * `url` - The URL to check
/// * `expected_sri` - Expected SRI hash in format "sha384-..."
///
/// # Returns
/// A `CheckResult` containing the outcome of the check
pub async fn check_resource(url: &str, expected_sri: &str) -> CheckResult {
    console_log!("Checking: {}", url);

    // Parse expected SRI
    let integrity = match expected_sri.parse::<Integrity>() {
        Ok(i) => i,
        Err(e) => {
            return CheckResult::failure(
                url.to_string(),
                format!("Invalid SRI format: {}", e),
            );
        }
    };

    // Fetch the resource
    let mut response = match fetch_resource(url).await {
        Ok(r) => r,
        Err(e) => {
            return CheckResult::failure(url.to_string(), format!("Fetch failed: {}", e));
        }
    };

    let status_code = response.status_code();

    // Check if response is successful (2xx status codes)
    if !(200..300).contains(&status_code) {
        return CheckResult::failure(
            url.to_string(),
            format!("HTTP error: {}", status_code),
        );
    }

    // Get response body
    let content = match response.bytes().await {
        Ok(c) => c,
        Err(e) => {
            return CheckResult::failure(
                url.to_string(),
                format!("Failed to read response body: {}", e),
            );
        }
    };

    // Verify SRI hash
    let sri_valid = match integrity.check(&content) {
        Ok(_) => {
            console_log!("✓ {} - SRI valid", url);
            true
        }
        Err(_) => {
            console_error!("✗ {} - SRI MISMATCH", url);
            false
        }
    };

    CheckResult::success(url.to_string(), status_code, sri_valid)
}

/// Fetch a resource from the given URL using HTTP GET
async fn fetch_resource(url: &str) -> Result<Response> {
    let url_parsed = url
        .parse()
        .map_err(|e| Error::RustError(format!("Invalid URL: {}", e)))?;

    let response = Fetch::Url(url_parsed).send().await?;

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_has_problem() {
        let success = CheckResult::success("https://example.com".to_string(), 200, true);
        assert!(!success.has_problem());

        let sri_fail = CheckResult::success("https://example.com".to_string(), 200, false);
        assert!(sri_fail.has_problem());

        let failure = CheckResult::failure(
            "https://example.com".to_string(),
            "Network error".to_string(),
        );
        assert!(failure.has_problem());
    }

    #[test]
    fn test_check_result_description() {
        let success = CheckResult::success("https://example.com".to_string(), 200, true);
        assert_eq!(success.description(), "OK (HTTP 200)");

        let sri_fail = CheckResult::success("https://example.com".to_string(), 200, false);
        assert_eq!(sri_fail.description(), "SRI mismatch (HTTP 200)");

        let failure = CheckResult::failure(
            "https://example.com".to_string(),
            "Network error".to_string(),
        );
        assert_eq!(failure.description(), "Failed: Network error");
    }
}
