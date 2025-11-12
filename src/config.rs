//! Configuration module with compile-time TOML parsing
//!
//! This module uses static_toml to parse config.toml at compile time,
//! generating native Rust types with zero runtime overhead.

use static_toml::static_toml;

// Parse config.toml at compile time and generate types
static_toml! {
    /// Compile-time parsed configuration from config.toml
    #[static_toml(prefer_slices = true)]
    pub static CONFIG = include_toml!("config.toml");
}

// Re-export the generated Resource type
// static_toml generates: config::resources::values::Values
pub use config::resources::values::Values as Resource;

/// Get the configuration version
pub fn version() -> &'static str {
    CONFIG.version
}

/// Get all configured resources to monitor
pub fn resources() -> &'static [Resource] {
    &CONFIG.resources
}

/// Get the number of resources configured
pub fn resource_count() -> usize {
    CONFIG.resources.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_loads() {
        assert_eq!(version(), "1.0");
        assert!(resource_count() > 0);
    }

    #[test]
    fn test_resources_have_url_and_sri() {
        for resource in resources() {
            assert!(!resource.url.is_empty(), "Resource URL should not be empty");
            assert!(!resource.sri.is_empty(), "Resource SRI should not be empty");
            assert!(
                resource.sri.starts_with("sha256-") 
                || resource.sri.starts_with("sha384-") 
                || resource.sri.starts_with("sha512-"),
                "SRI should start with valid algorithm prefix"
            );
        }
    }
}
