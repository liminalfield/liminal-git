// native/src/lib.rs

// Only compile NAPI service when napi-binding feature is enabled
#[cfg(feature = "napi-binding")]
mod git_service;

// Core modules always available for testing
pub mod errors;
pub mod feature_flags;
pub mod types;
pub mod utils;

// Validation is pure logic — string and filesystem checks — and returns
// GitError, so it is NOT gated. It used to be, because it returned
// napi::Error; that gating made its 50 tests unreachable, since a target
// linking napi cannot build as a standalone test binary (napi resolves its
// symbols from the host Node process at runtime). Conversion to napi::Error
// happens at the boundary in git_service.rs via `From`.
pub mod file_ops;
pub mod history_ops;
pub mod repository_ops;
pub mod validation;

// Branch and tag operations - core functionality always available
pub mod branch_ops;
pub mod tag_ops;

// Core service for testing without NAPI
mod core;

// Only export GitService when NAPI is enabled
#[cfg(feature = "napi-binding")]
pub use git_service::GitService;

// Always export types and operations for testing
pub use branch_ops::*;
pub use errors::*;
pub use feature_flags::*;
pub use file_ops::*;
pub use history_ops::*;
pub use repository_ops::*;
pub use tag_ops::*;
pub use types::*;

// Export core service for tests
pub use core::GitServiceCore;

// Shared test helpers live in tests/common/ and are pulled in with
// `mod common;`. There used to be a second copy at tests/test_utils.rs, which
// integration_tests.rs tried to reach as `liminal_git::test_utils` — a path
// that never existed, since files under tests/ are separate test binaries and
// not part of this crate. It has been deleted; tests/common/ was a strict
// superset of it.

// Simple test to verify GitServiceCore works without NAPI
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_service_core_creation() {
        let core = GitServiceCore::new();
        // Test basic functionality
        assert!(!core.is_valid_branch_name(""));
        assert!(core.is_valid_branch_name("main"));
        assert!(!core.is_valid_tag_name(""));
        assert!(core.is_valid_tag_name("v1.0.0"));
    }
}
