# Liminal-Field-Git

Rust/N-API native module providing Git operations for Node.js applications.

## Development

### Building

```bash
# Build the native module
npm run build

# Build TypeScript error helpers (generates errors.js and errors.d.ts)
npm run build:errors

# Or use cargo directly for Rust code
cargo build --release
```

**Important**: If you edit `errors.ts`, you must regenerate the JavaScript and TypeScript declarations:

```bash
npm run build:errors
```

This is automatically done before:
- Running tests (`npm test` or `npm run test:js`)
- Publishing (`prepublishOnly` hook)

**Module Format**: The package is CommonJS (main module uses `require()`), so `errors.ts` is compiled to CommonJS with `--module commonjs --esModuleInterop`. This ensures `require('liminal-field-git/errors')` works correctly.

### Testing

The module has two test suites:

#### Rust Tests

**Important:** Rust tests must be run with `--no-default-features` to avoid NAPI linking errors on Linux.

```bash
# Run all Rust tests (recommended)
cargo test --lib --no-default-features

# Run specific test module
cargo test --lib --no-default-features repository_ops

# Run with test output visible
cargo test --lib --no-default-features -- --nocapture
```

**Why `--no-default-features`?** The default `napi-binding` feature enables Node.js N-API bindings, which require Node's runtime symbols (`napi_reference_unref`, etc.) to be available during linking. These symbols are not present in the standard Rust test environment on Linux, causing link failures. Disabling the feature runs tests against the pure Rust implementation layer.

**Serial Execution Note**: Branch operation tests modify global `TMPDIR` environment variables to fix cross-device link issues on WSL/mounted filesystems. While tests are marked `#[serial_test::serial]` to prevent concurrent modifications, running the full suite with `--test-threads=1` is recommended for reliability:

```bash
# Recommended for full suite
cargo test --lib --no-default-features -- --test-threads=1

# Individual test modules can run normally
cargo test --lib --no-default-features file_ops
```

#### JavaScript/TypeScript Tests

Tests for the error handling module (`errors.ts`) use Jest:

```bash
# Run JavaScript tests only
npm run test:js

# Run both Rust and JavaScript tests
npm test

# Run with coverage
npm run test:js -- --coverage
```

The JavaScript test suite (`errors.test.ts`) validates:
- JSON parsing and validation in `parseStructuredGitError()`
- Type guard behavior for all error codes
- Type narrowing with TypeScript
- Edge case handling (invalid JSON, missing fields, etc.)

## Feature Flags

Control experimental features via the `LIMINAL_FEATURE_FLAGS` environment variable:

```bash
export LIMINAL_FEATURE_FLAGS=structured_errors,enhanced_status
```

### Available Flags

- `structured_errors` - Return structured JSON errors with typed details (default: off)
- `enhanced_status` - Enhanced git status with detailed metadata (default: off)
- `enhanced_diff` - Populated diff hunks with line content (default: off)

## Operations

### Phase 3 Tier 1: Critical Git Operations

The following operations are available (implemented in Phase 3 Tier 1):

#### File Operations

**`discard_changes(repoPath, filePath)`**
- Discards uncommitted changes to a file, reverting to HEAD state
- Removes from staging area if staged
- Validates file exists in HEAD
- Errors: `FILE_NOT_IN_REPOSITORY`, `FILE_NOT_FOUND`, `DETACHED_HEAD`

**`unstage_file(repoPath, filePath)`**
- Removes file from staging area (keeps working directory changes)
- Safely handles empty repository and bare repository cases
- No-op if file not staged
- Errors: `FILE_NOT_IN_REPOSITORY`, `EMPTY_REPOSITORY`, `BARE_REPOSITORY`

**`commit_amend(repoPath, message, userName?, userEmail?)`**
- Amends the most recent commit with staged changes and/or new message
- Config fallback: Explicit params → repo config → global config → error
- Preserves original commit authorship
- Errors: `NOTHING_TO_COMMIT`, `DETACHED_HEAD`, `EMPTY_REPOSITORY`, `CONFIG_MISSING`

#### Branch Operations

**`checkout_branch(repoPath, branchName)`**
- Smart branch switching with config-driven conflict handling
- **Precise conflict detection**: Only blocks when files would actually be overwritten
- Config strategy via `liminal.checkoutStrategy` ("safe" default, "force" opt-in)
- Returns exact list of conflicting files (not all dirty files)
- Errors: `BRANCH_NOT_FOUND`, `UNSTAGED_CHANGES_WOULD_BE_LOST`

**Configuration**:
```bash
# Safe mode (default): blocks only on actual conflicts
git config liminal.checkoutStrategy "safe"

# Force mode (dangerous): overwrites local changes
git config liminal.checkoutStrategy "force"
```

**`create_branch(repoPath, branchName)`**
- Creates a new branch at current HEAD
- Errors: `BRANCH_ALREADY_EXISTS`

**`delete_branch(repoPath, branchName, force)`**
- Deletes a local branch with safety checks
- Validates branch is merged (unless force=true)
- Errors: `BRANCH_NOT_FOUND`, `CANNOT_DELETE_CURRENT_BRANCH`, `BRANCH_NOT_MERGED`

**`list_branches(repoPath)`**
- Lists all local branches with metadata
- Returns: name, commit hash, ahead/behind counts, last updated

#### Tag Operations

**`create_tag(repoPath, options)`**
- Creates lightweight or annotated tags
- Config-aware signatures for annotated tags
- Options: `{ name, message?, user_name?, user_email?, force?, target_commit? }`
- Config fallback chain for signatures (annotated only)
- Errors: `TAG_ALREADY_EXISTS`, `INVALID_TAG_NAME`, `CONFIG_MISSING`

**`delete_tag(repoPath, tagName)`**
- Deletes a tag
- Errors: `TAG_NOT_FOUND`

**`list_tags(repoPath)`**
- Lists all tags with metadata
- Returns: name, commit hash, message, tagger, created timestamp

See [`docs/archive/MIGRATION_PHASE3.md`](../../../docs/archive/MIGRATION_PHASE3.md) for detailed operation documentation, error handling patterns, and usage examples.

## Structured Errors

When `structured_errors` flag is enabled, Git errors are returned as JSON payloads with typed information:

```json
{
  "code": "FILE_NOT_FOUND",
  "message": "File not found: /path/to/file",
  "retriable": false,
  "details": {
    "path": "/path/to/file"
  }
}
```

### Error Codes

All error codes are stable and documented:

- **Repository**: `REPOSITORY_NOT_FOUND`, `REPOSITORY_CORRUPTED`, `INVALID_REPOSITORY`
- **Files**: `FILE_NOT_FOUND`, `FILE_NOT_IN_REPOSITORY`, `PATH_TRAVERSAL`
- **Operations**: `NOTHING_TO_COMMIT`, `MERGE_CONFLICT`, `UNCOMMITTED_CHANGES`, `DETACHED_HEAD`, `UNSTAGED_CHANGES_WOULD_BE_LOST`, `CONFIG_MISSING`
- **Branches**: `BRANCH_NOT_FOUND`, `BRANCH_ALREADY_EXISTS`, `CANNOT_DELETE_CURRENT_BRANCH`, `BRANCH_NOT_MERGED`
- **Tags**: `TAG_NOT_FOUND`, `TAG_ALREADY_EXISTS`
- **Validation**: `INVALID_PATH`, `INVALID_COMMIT_HASH`, `INVALID_BRANCH_NAME`, `INVALID_TAG_NAME`
- **System**: `IO_ERROR`, `GIT_OPERATION_FAILURE`

### Usage in TypeScript/JavaScript

Import the error handling helpers from the companion `errors` module:

```typescript
import { GitService } from 'liminal-field-git';
import {
  parseStructuredGitError,
  isFileNotFoundError,
  isBranchError,
  type StructuredGitError,
  type FileNotFoundError,
} from 'liminal-field-git/errors';

const gitService = new GitService();

try {
  await gitService.commitFile(path, message, user, email);
} catch (err) {
  const structured = parseStructuredGitError(err);

  if (structured) {
    // Structured error with typed details
    console.log(`Error ${structured.code}: ${structured.message}`);

    if (isFileNotFoundError(structured)) {
      // TypeScript narrows type to FileNotFoundError
      console.log(`File missing: ${structured.details.path}`);
    }

    if (structured.retriable) {
      // Safe to retry
      console.log('This error is retryable');
    }
  } else {
    // Legacy unstructured error (flag disabled)
    console.error('Legacy error:', err);
  }
}
```

**Available exports from `liminal-field-git/errors`**:

- `parseStructuredGitError(err)` - Parse structured error from exception
- Type guards: `isFileNotFoundError`, `isBranchNotFoundError`, `isMergeConflictError`, etc.
- Category guards: `isRepositoryError`, `isFileError`, `isBranchError`, `isTagError`
- `isRetryableError(err)` - Check if error is safe to retry
- TypeScript interfaces: `StructuredGitError`, `FileNotFoundError`, `BranchNotFoundError`, etc.

### Implementation Details

**JSON Transport Constraint**: napi-rs 3.3 only supports `napi::Error::new(status, message)` - there's no way to attach arbitrary properties to error objects. We serialize the complete error structure to JSON for transport, then parse it on the JavaScript side.

The `GitError::build_details_object()` method exists for future upgrade when napi-rs adds native structured error support, but is currently dormant. When that support arrives, we can switch to attaching properties directly without changing the variant logic or JavaScript consumer API.

**Future-Proofing**: Both serialization paths (`to_serializable()` for JSON transport and `build_details_object()` for future native objects) use exhaustive matches on all GitError variants. The compiler will error if new variants are added without updating both methods, ensuring consistency.

### Format

Comma-separated exact tokens (case-insensitive, no partial matching):

```bash
# ✓ Correct - enables both flags
LIMINAL_FEATURE_FLAGS=structured_errors,enhanced_status

# ✓ Correct - case insensitive
LIMINAL_FEATURE_FLAGS=STRUCTURED_ERRORS,Enhanced_Status

# ✗ Wrong - partial match does NOT work
LIMINAL_FEATURE_FLAGS=structured_errors_off  # Does NOT enable structured_errors
```

## Logging

Enable debug logging to track operations:

```bash
export LIMINAL_LOG=1
npm test  # or run your application
```

Logs include:
- Operation name
- File paths
- Duration
- Outcome (success/error)

Output goes to stderr in format: `[level] operation=name path=/path/to/file duration_ms=42`

## Coverage

### Prerequisites

```bash
# Install coverage tool (one time)
cargo install cargo-llvm-cov --version 0.6.17
```

### Running Coverage

```bash
# Generate HTML coverage report
cargo llvm-cov --no-default-features --lib --html

# Open report
open target/llvm-cov/html/index.html  # macOS
xdg-open target/llvm-cov/html/index.html  # Linux

# Terminal summary only
cargo llvm-cov --no-default-features --lib
```

### Coverage Baseline

- **Date:** 2025-01-13 (Phase 3 Tier 1 Complete)
- **Tool:** cargo-llvm-cov 0.6.17
- **Line Coverage:** 45.78% (1,525 / 3,331 lines)
- **Function Coverage:** 27.31% (118 / 432 functions)
- **Region Coverage:** 47.93% (3,228 / 6,735 regions)
- **Command:** `cargo llvm-cov --no-default-features --lib`

**Module Coverage** (Phase 3 Tier 1):
- **branch_ops.rs**: 41.49% line coverage (target: ≥35%) ✓
- **file_ops.rs**: 54.61% line coverage (target: ≥50%) ✓
- **tag_ops.rs**: 33.57% line coverage (target: ≥30%) ✓
- **repository_ops.rs**: 20.58% line coverage (target: ≥20%) ✓
- **errors.rs**: 65.47% line coverage ✓
- **feature_flags.rs**: 100.00% line coverage ✓

**Test Count**: 56 tests (up from 51 in Phase 2)

**Note:** Significant coverage improvement from Phase 1 baseline (8.15%) due to comprehensive test suite for Phase 3 Tier 1 operations (discard_changes, commit_amend, smart checkout, enhanced tagging).

## Architecture

### Module Structure

```
src/
├── lib.rs              - Module exports and feature flags
├── types.rs            - TypeScript-compatible data structures
├── feature_flags.rs    - Feature flag system
├── git_service.rs      - NAPI bindings layer
├── core.rs             - Non-NAPI service for testing
├── validation.rs       - Input validation and sanitization
├── utils.rs            - Path normalization helpers
├── repository_ops.rs   - Repository operations (init, status, config)
├── file_ops.rs         - File operations (stage, commit, move)
├── history_ops.rs      - History and diff operations
├── branch_ops.rs       - Branch management
└── tag_ops.rs          - Tag management
```

### Design Principles

1. **Separation of Concerns**: NAPI bindings (`git_service.rs`) separate from core logic (`*_ops.rs`)
2. **Feature Flags**: Safe rollback mechanism for breaking changes
3. **Path Safety**: All paths validated and normalized before use
4. **Testing**: Core logic testable without Node.js via `core.rs`

## Contributing

### Code Style

```bash
# Format code
cargo fmt

# Run linter
cargo clippy --all-targets
```

### Adding Tests

- Unit tests: Add `#[cfg(test)]` module in source file
- Integration tests: Add file in `tests/` directory
- Use `serial_test` crate for tests that cannot run concurrently

## License

[Your license here]
