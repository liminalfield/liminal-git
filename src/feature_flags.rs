use std::collections::HashSet;

/// Feature flags to control experimental functionality
///
/// Flags are enabled via the LIMINAL_FEATURE_FLAGS environment variable
/// Format: comma-separated list (e.g., "structured_errors,enhanced_status")
/// Matching is case-insensitive and requires exact tokens
#[derive(Debug, Clone)]
pub struct FeatureFlags {
    /// Return structured JSON errors instead of plain strings
    pub structured_errors: bool,

    /// Enhanced git status with detailed metadata
    pub enhanced_status: bool,

    /// Populated diff hunks with line content
    pub enhanced_diff: bool,
}

impl FeatureFlags {
    /// Parse feature flags from LIMINAL_FEATURE_FLAGS environment variable
    ///
    /// Example:
    /// ```
    /// use liminal_git::feature_flags::FeatureFlags;
    ///
    /// // set_var is unsafe as of edition 2024: it mutates process-global
    /// // state that other threads may be reading. A doc test is a process of
    /// // its own, so this is sound here.
    /// unsafe { std::env::set_var("LIMINAL_FEATURE_FLAGS", "structured_errors,enhanced_status") };
    ///
    /// let flags = FeatureFlags::from_env();
    /// assert!(flags.structured_errors);
    /// assert!(flags.enhanced_status);
    /// assert!(!flags.enhanced_diff);
    /// ```
    pub fn from_env() -> Self {
        let flags_str = std::env::var("LIMINAL_FEATURE_FLAGS")
            .unwrap_or_default();

        // Parse comma-separated tokens into HashSet for exact matching
        // Normalize to lowercase to avoid case sensitivity issues
        let flags: HashSet<String> = flags_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            structured_errors: flags.contains("structured_errors"),
            enhanced_status: flags.contains("enhanced_status"),
            enhanced_diff: flags.contains("enhanced_diff"),
        }
    }

    /// Create flags with all features disabled (default/safe mode)
    pub fn disabled() -> Self {
        Self {
            structured_errors: false,
            enhanced_status: false,
            enhanced_diff: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_flags_from_env_empty() {
        unsafe {
            std::env::remove_var("LIMINAL_FEATURE_FLAGS");
        }
        let flags = FeatureFlags::from_env();
        assert!(!flags.structured_errors);
        assert!(!flags.enhanced_status);
        assert!(!flags.enhanced_diff);
    }

    #[test]
    #[serial]
    fn test_flags_single_flag() {
        unsafe {
            std::env::set_var("LIMINAL_FEATURE_FLAGS", "structured_errors");
        }
        let flags = FeatureFlags::from_env();
        assert!(flags.structured_errors);
        assert!(!flags.enhanced_status);
        assert!(!flags.enhanced_diff);
    }

    #[test]
    #[serial]
    fn test_flags_multiple_flags() {
        unsafe {
            std::env::set_var("LIMINAL_FEATURE_FLAGS", "structured_errors,enhanced_status");
        }
        let flags = FeatureFlags::from_env();
        assert!(flags.structured_errors);
        assert!(flags.enhanced_status);
        assert!(!flags.enhanced_diff);
    }

    #[test]
    #[serial]
    fn test_flags_case_insensitive() {
        unsafe {
            std::env::set_var("LIMINAL_FEATURE_FLAGS", "STRUCTURED_ERRORS,Enhanced_Status");
        }
        let flags = FeatureFlags::from_env();
        assert!(flags.structured_errors);
        assert!(flags.enhanced_status);
        assert!(!flags.enhanced_diff);
    }

    #[test]
    #[serial]
    fn test_flags_no_partial_match() {
        unsafe {
            std::env::set_var("LIMINAL_FEATURE_FLAGS", "structured_errors_off,enhanced_status_disabled");
        }
        let flags = FeatureFlags::from_env();
        // Should NOT match - requires exact tokens
        assert!(!flags.structured_errors);
        assert!(!flags.enhanced_status);
    }

    #[test]
    #[serial]
    fn test_flags_whitespace_handling() {
        unsafe {
            std::env::set_var("LIMINAL_FEATURE_FLAGS", " structured_errors , enhanced_status ");
        }
        let flags = FeatureFlags::from_env();
        assert!(flags.structured_errors);
        assert!(flags.enhanced_status);
    }

    #[test]
    fn test_disabled_constructor() {
        let flags = FeatureFlags::disabled();
        assert!(!flags.structured_errors);
        assert!(!flags.enhanced_status);
        assert!(!flags.enhanced_diff);
    }
}
