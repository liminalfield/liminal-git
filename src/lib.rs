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

// There used to be a `GitServiceCore` here: a 186-line struct whose every
// method forwarded to a `*_impl` function and re-wrapped the error in
// `anyhow`. Its stated purpose was "testing without NAPI dependencies" — it
// existed only because the ops modules were once gated behind the napi
// feature and so unreachable from a test binary. That gating is gone, tests
// call the `*_impl` functions directly, and nothing referenced the shim.
// Deleting it removed the crate's last use of `anyhow`.

// Shared test helpers live in tests/common/ and are pulled in with
// `mod common;`. There used to be a second copy at tests/test_utils.rs, which
// integration_tests.rs tried to reach as `liminal_git::test_utils` — a path
// that never existed, since files under tests/ are separate test binaries and
// not part of this crate. It has been deleted; tests/common/ was a strict
// superset of it.
