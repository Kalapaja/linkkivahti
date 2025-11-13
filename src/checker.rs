//! Link availability and SRI verification module

use ssri::Integrity;
use worker::*;

/// Typed error for check failures
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckError {
    /// Invalid SRI format in configuration
    InvalidSri,
    /// Network request failed
    FetchFailed,
    /// HTTP error response, with code
    HttpError(u16),
    /// Failed to read response body
    BodyReadFailed,
}

impl CheckError {
    /// Get a human-readable description of the error
    #[inline]
    pub fn description(&self) -> String {
        match self {
            Self::InvalidSri => "Invalid SRI format".to_string(),
            Self::FetchFailed => "Fetch failed".to_string(),
            Self::HttpError(code) => format!("HTTP error: {}", code),
            Self::BodyReadFailed => "Failed to read response body".to_string(),
        }
    }
}

/// Result of a link check operation
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub url: &'static str,
    pub success: bool,
    pub status_code: Option<u16>,
    pub error: Option<CheckError>,
    pub sri_valid: Option<bool>,
}

impl CheckResult {
    /// Create a successful check result
    #[inline]
    pub fn success(url: &'static str, status_code: u16, sri_valid: bool) -> Self {
        Self {
            url,
            success: true,
            status_code: Some(status_code),
            error: None,
            sri_valid: Some(sri_valid),
        }
    }

    /// Create a failed check result
    #[inline]
    pub fn failure(url: &'static str, error: CheckError) -> Self {
        Self {
            url,
            success: false,
            status_code: None,
            error: Some(error),
            sri_valid: None,
        }
    }

    /// Check if this result indicates a problem (failure or SRI mismatch)
    #[inline]
    pub fn has_problem(&self) -> bool {
        !self.success || self.sri_valid == Some(false)
    }

    /// Get a human-readable description of the result
    pub fn description(&self) -> String {
        if !self.success {
            if let Some(error) = &self.error {
                format!("Failed: {}", error.description())
            } else {
                "Failed: Unknown error".to_string()
            }
        } else if self.sri_valid == Some(false) {
            match self.status_code {
                Some(code) => format!("SRI mismatch (HTTP {})", code),
                None => "SRI mismatch".to_string(),
            }
        } else {
            match self.status_code {
                Some(code) => format!("OK (HTTP {})", code),
                None => "OK".to_string(),
            }
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
pub async fn check_resource(url: &'static str, expected_sri: &str) -> CheckResult {
    console_log!("Checking: {}", url);

    // Parse expected SRI - use borrowed string on success path
    let integrity = match expected_sri.parse::<Integrity>() {
        Ok(i) => i,
        Err(_) => {
            return CheckResult::failure(url, CheckError::InvalidSri);
        }
    };

    // Fetch the resource
    let mut response = match fetch_resource(url).await {
        Ok(r) => r,
        Err(_) => {
            return CheckResult::failure(url, CheckError::FetchFailed);
        }
    };

    let status_code = response.status_code();

    // Check if response is successful (2xx status codes)
    // Fail fast before reading body
    if !(200..300).contains(&status_code) {
        return CheckResult::failure(url, CheckError::HttpError(status_code));
    }

    // Get response body
    let content = match response.bytes().await {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::failure(url, CheckError::BodyReadFailed);
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

    CheckResult::success(url, status_code, sri_valid)
}

/// Fetch a resource from the given URL using HTTP GET
#[inline]
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
    fn test_check_error_description() {
        assert_eq!(CheckError::InvalidSri.description(), "Invalid SRI format");
        assert_eq!(CheckError::FetchFailed.description(), "Fetch failed");
        assert_eq!(CheckError::HttpError(404).description(), "HTTP error: 404");
    }

    #[test]
    fn test_check_result_has_problem() {
        let success = CheckResult::success("https://example.com", 200, true);
        assert!(!success.has_problem());

        let sri_fail = CheckResult::success("https://example.com", 200, false);
        assert!(sri_fail.has_problem());

        let failure = CheckResult::failure("https://example.com", CheckError::FetchFailed);
        assert!(failure.has_problem());
    }

    #[test]
    fn test_check_result_description() {
        let success = CheckResult::success("https://example.com", 200, true);
        assert_eq!(success.description(), "OK (HTTP 200)");

        let sri_fail = CheckResult::success("https://example.com", 200, false);
        assert_eq!(sri_fail.description(), "SRI mismatch (HTTP 200)");

        let failure = CheckResult::failure("https://example.com", CheckError::FetchFailed);
        assert_eq!(failure.description(), "Failed: Fetch failed");

        let http_error = CheckResult::failure("https://example.com", CheckError::HttpError(404));
        assert_eq!(http_error.description(), "Failed: HTTP error: 404");
    }
}
