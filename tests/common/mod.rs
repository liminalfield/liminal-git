// tests/common/mod.rs - Shared test utilities and helpers

// Every test target that says `mod common;` compiles this whole file, then
// uses a slice of it — so each target warns about the rest. The warnings say
// nothing about the code and drown the ones that do.
#![allow(dead_code, unused_imports)]

pub mod assertions;
pub mod fixtures;
pub mod test_repo;

// Re-export commonly used types and functions
pub use assertions::*;
pub use fixtures::*;
pub use test_repo::TestRepo;

// Common imports that every test needs
pub use std::fs;
pub use tempfile::TempDir;

// Re-export all operation functions for easy access in tests.
//
// branch_ops and tag_ops are NOT gated behind napi-binding. Only the async
// napi wrappers inside them are; the `*_impl` functions that these tests
// actually exercise are plain Rust. The gate that used to sit here hid them
// from the only profile these tests can run under — a test binary cannot link
// napi, which resolves its symbols from the host Node process at runtime.
pub use liminal_git::{
    branch_ops::*, errors::*, file_ops::*, history_ops::*, remote_ops::*, repository_ops::*,
    tag_ops::*, types::*, validation::*,
};
