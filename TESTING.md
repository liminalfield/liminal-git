# Testing Framework Documentation

This document describes the comprehensive testing framework for the Rust Git Service used in Nocturne Writer.

## Overview

The testing framework includes:
- **Unit Tests**: Individual function testing with isolated scenarios
- **Integration Tests**: End-to-end workflow testing
- **Performance Benchmarks**: Performance profiling and regression detection
- **Security Tests**: Input validation and path traversal protection
- **Test Utilities**: Shared testing infrastructure and fixtures

## Quick Start

### Running All Tests

```bash
# Linux/macOS
./run_tests.sh

# Windows
run_tests.bat
```

### Running Specific Test Types

```bash
# Unit tests only
cargo test --lib

# Integration tests only
cargo test --test integration_tests

# Benchmarks only
cargo bench

# With coverage report
./run_tests.sh --coverage
```

### Quick Mode (Essential Tests Only)

```bash
./run_tests.sh --quick
```

## Test Structure

```
native/
├── src/
│   ├── test_utils.rs          # Shared test utilities
│   ├── validation.rs          # Unit tests for validation
│   └── operations.rs          # Unit tests for operations
├── tests/
│   ├── integration_tests.rs   # End-to-end integration tests
│   └── fixtures/
│       ├── create_repos.rs    # Test repository creation
│       └── test_data.rs       # Test data constants
├── benches/
│   └── git_operations.rs      # Performance benchmarks
├── test-fixtures/             # Pre-created test repositories
│   ├── empty-repo/
│   ├── simple-repo/
│   └── complex-repo/
└── run_tests.sh              # Test execution script
```

## Test Categories

### Unit Tests

Located in individual module files (`src/*.rs`), these test individual functions in isolation:

- **Validation Tests**: Input validation, path traversal protection, boundary conditions
- **Operation Tests**: Git operations, error handling, edge cases
- **Utility Tests**: Helper functions, path manipulation, error conversion

Example:
```rust
#[test]
fn test_validate_file_path_traversal_attempts() {
    let result = validate_file_path("../outside.txt");
    assert!(result.is_err());
}
```

### Integration Tests

Located in `tests/integration_tests.rs`, these test complete workflows:

- **Complete Git Workflows**: Repository creation → file addition → staging → committing
- **Error Scenarios**: Invalid repositories, missing files, validation failures
- **Performance Tests**: Large repositories, many files, concurrent operations
- **Cross-Platform Tests**: Unicode filenames, nested directories, binary files

Example:
```rust
#[test]
fn test_complete_git_workflow() {
    let test_repo = TestRepo::new().unwrap();
    let git_service = GitService::new();

    // Add files → stage → commit → verify clean
}
```

### Performance Benchmarks

Located in `benches/git_operations.rs`, these test performance characteristics:

- **Status Operations**: Repository scanning with varying file counts
- **Staging Operations**: File staging performance at scale
- **Commit Operations**: Commit creation with different payload sizes
- **Memory Usage**: Memory leak detection and resource management
- **Concurrency**: Thread safety and concurrent operation performance

Example:
```rust
fn bench_git_status(c: &mut Criterion) {
    for file_count in [10, 100, 500, 1000].iter() {
        // Benchmark status operation with different repository sizes
    }
}
```

## Test Utilities

### TestRepo

Main utility for creating temporary Git repositories:

```rust
let test_repo = TestRepo::new().unwrap();
test_repo.add_file("test.txt", "content").unwrap();
test_repo.stage_file("test.txt").unwrap();
test_repo.commit("Initial commit").unwrap();
```

### Assertion Helpers

Specialized assertions for Git operations:

```rust
assertions::assert_git_status_matches(&status, 1, 2, 0); // modified, untracked, staged
assertions::assert_commit_exists(&repo, "commit message");
assertions::assert_file_staged(&repo, "file.txt");
```

### Test Fixtures

Pre-created repositories for complex scenarios:

- **empty-repo**: Freshly initialized repository
- **simple-repo**: Single commit with basic files
- **complex-repo**: Multiple commits, mixed file states

## Configuration

### Environment Variables

- `RUST_TEST_THREADS`: Number of test threads (default: CPU cores)
- `RUST_LOG`: Logging level for test output

### Test Execution Options

```bash
# Quick mode - essential tests only
./run_tests.sh --quick

# Generate coverage report
./run_tests.sh --coverage

# Skip benchmarks
./run_tests.sh --no-benchmarks

# Skip fixture creation
./run_tests.sh --no-fixtures
```

## Performance Targets

The testing framework validates these performance requirements:

- **Status Check**: <50ms for repositories with 1000+ files
- **Single Commit**: <100ms
- **Multi-file Commit**: <200ms regardless of file count
- **Memory Usage**: <50MB for typical operations
- **Concurrency**: Thread-safe operations without data races

## Security Testing

Security tests validate protection against:

- **Path Traversal**: `../`, `..\\`, absolute paths outside repository
- **Input Validation**: Null bytes, oversized inputs, malformed data
- **Resource Exhaustion**: Maximum file counts, memory limits
- **Concurrent Access**: Race conditions, index.lock conflicts

## Continuous Integration

The test suite is designed for CI/CD environments:

```yaml
# Example GitHub Actions configuration
- name: Run Test Suite
  run: |
    cd native
    ./run_tests.sh --quick

- name: Run Full Test Suite
  run: |
    cd native
    ./run_tests.sh --coverage
```

## Troubleshooting

### Common Issues

**Tests fail with "repository not found"**:
- Ensure test fixtures are created: `cargo test fixtures::create_repos::create_all_fixtures`

**Permission errors on Windows**:
- Run command prompt as administrator
- Check Windows Defender exclusions

**Memory errors with valgrind**:
- Install valgrind: `sudo apt-get install valgrind`
- Use `--quick` mode to skip memory testing

**Benchmark failures**:
- Ensure stable system load during benchmarking
- Use `--no-benchmarks` flag to skip performance tests

### Debug Mode

Enable verbose logging:
```bash
RUST_LOG=debug cargo test
```

Run specific test:
```bash
cargo test test_complete_git_workflow -- --nocapture
```

## Adding New Tests

### Unit Test

Add to the appropriate module file:
```rust
#[test]
fn test_new_feature() {
    // Test implementation
}
```

### Integration Test

Add to `tests/integration_tests.rs`:
```rust
#[serial]
#[test]
fn test_new_workflow() {
    let test_repo = TestRepo::new().unwrap();
    // Integration test implementation
}
```

### Benchmark

Add to `benches/git_operations.rs`:
```rust
fn bench_new_operation(c: &mut Criterion) {
    c.bench_function("new_operation", |b| {
        b.iter(|| {
            // Benchmark implementation
        });
    });
}
```

## Best Practices

1. **Use `TestRepo`**: Always use the test utilities for repository creation
2. **Clean Up**: Tests automatically clean up temporary files
3. **Isolation**: Use `#[serial]` for tests that can't run in parallel
4. **Performance**: Include timing assertions for critical operations
5. **Error Testing**: Test both success and failure paths
6. **Documentation**: Document complex test scenarios

## References

- [Rust Testing Guide](https://doc.rust-lang.org/book/ch11-00-testing.html)
- [Criterion.rs Benchmarking](https://bheisler.github.io/criterion.rs/book/)
- [Serial Test Crate](https://docs.rs/serial_test/)
- [Git2 Documentation](https://docs.rs/git2/)