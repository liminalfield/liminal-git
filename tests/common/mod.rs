// tests/common/mod.rs - Shared test utilities and helpers

pub mod test_repo;
pub mod assertions;
pub mod fixtures;

// Re-export commonly used types and functions
pub use test_repo::TestRepo;
pub use assertions::*;
pub use fixtures::*;

// Common imports that every test needs
pub use std::fs;
pub use tempfile::TempDir;

// Re-export all operation functions for easy access in tests
pub use liminal_field_git::{
    file_ops::*,
    repository_ops::*,
    history_ops::*,
    types::*,
};

// Branch and tag operations only available with NAPI feature
#[cfg(feature = "napi-binding")]
pub use liminal_field_git::{
    branch_ops::*,
    tag_ops::*,
};