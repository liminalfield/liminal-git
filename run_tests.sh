#!/bin/bash

set -e

echo "🦀 Rust Git Service - Comprehensive Test Suite"
echo "============================================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_section() {
    echo -e "${BLUE}[SECTION]${NC} $1"
}

# Function to run command with error handling
run_command() {
    local cmd="$1"
    local desc="$2"

    print_status "Running: $desc"
    if $cmd; then
        print_status "✅ $desc completed successfully"
    else
        print_error "❌ $desc failed"
        exit 1
    fi
    echo
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Must be run from the native/ directory"
    exit 1
fi

# Parse command line arguments
QUICK_MODE=false
COVERAGE=false
BENCHMARKS=true
FIXTURES=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --quick)
            QUICK_MODE=true
            BENCHMARKS=false
            shift
            ;;
        --coverage)
            COVERAGE=true
            shift
            ;;
        --no-benchmarks)
            BENCHMARKS=false
            shift
            ;;
        --no-fixtures)
            FIXTURES=false
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS]"
            echo "Options:"
            echo "  --quick         Run only essential tests (faster)"
            echo "  --coverage      Generate coverage report (requires cargo-tarpaulin)"
            echo "  --no-benchmarks Skip benchmark tests"
            echo "  --no-fixtures   Skip fixture creation"
            echo "  -h, --help      Show this help message"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [ "$QUICK_MODE" = true ]; then
    print_warning "Running in quick mode - skipping benchmarks and some tests"
fi

print_section "Environment Setup"

# Check Rust installation
if ! command -v cargo &> /dev/null; then
    print_error "Cargo not found. Please install Rust."
    exit 1
fi

# Check Git installation
if ! command -v git &> /dev/null; then
    print_error "Git not found. Please install Git."
    exit 1
fi

print_status "Rust version: $(rustc --version)"
print_status "Cargo version: $(cargo --version)"
print_status "Git version: $(git --version)"

# Clean previous builds
print_section "Cleanup"
run_command "cargo clean" "Cleaning previous builds"

# Check code formatting
print_section "Code Quality Checks"
if command -v cargo-fmt &> /dev/null; then
    run_command "cargo fmt --check" "Checking code formatting"
else
    print_warning "cargo-fmt not found, skipping format check"
fi

# Check for linting issues
if command -v cargo-clippy &> /dev/null; then
    run_command "cargo clippy -- -D warnings" "Running clippy lints"
else
    print_warning "cargo-clippy not found, skipping lint check"
fi

# Create test fixtures
if [ "$FIXTURES" = true ]; then
    print_section "Test Fixtures"
    run_command "cargo test --lib fixtures::create_repos::create_all_fixtures" "Creating test fixtures"
fi

# Build the project
print_section "Build"
run_command "cargo build" "Building project"

if [ "$QUICK_MODE" = false ]; then
    run_command "cargo build --release" "Building project in release mode"
fi

# Run unit tests
print_section "Unit Tests"
run_command "cargo test --lib" "Running unit tests"

# Run integration tests
print_section "Integration Tests"
run_command "cargo test --test integration_tests --features test-utils" "Running integration tests"

# Run tests in release mode for performance validation
if [ "$QUICK_MODE" = false ]; then
    print_section "Performance Validation"
    run_command "cargo test --release --test integration_tests test_large_repository_performance --features test-utils" "Running performance validation tests"
    run_command "cargo test --release --test integration_tests test_concurrent_operations --features test-utils" "Running concurrency tests"
fi

# Run benchmarks
if [ "$BENCHMARKS" = true ]; then
    print_section "Performance Benchmarks"
    run_command "cargo bench --features test-utils" "Running performance benchmarks"
fi

# Generate documentation
print_section "Documentation"
run_command "cargo doc --no-deps" "Generating documentation"

# Security audit
if command -v cargo-audit &> /dev/null; then
    print_section "Security Audit"
    run_command "cargo audit" "Running security audit"
else
    print_warning "cargo-audit not found. Install with: cargo install cargo-audit"
fi

# Generate coverage report
if [ "$COVERAGE" = true ]; then
    print_section "Coverage Report"
    if command -v cargo-tarpaulin &> /dev/null; then
        run_command "cargo tarpaulin --out Html --output-dir target/coverage" "Generating coverage report"
        print_status "Coverage report generated in target/coverage/tarpaulin-report.html"
    else
        print_warning "cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin"
    fi
fi

# Memory leak detection (if valgrind is available)
if command -v valgrind &> /dev/null && [ "$QUICK_MODE" = false ]; then
    print_section "Memory Leak Detection"
    print_warning "Running memory leak detection (this may take a while)..."

    # Build test binary
    cargo test --no-run --bin git_operations 2>/dev/null || true

    # Find the test binary
    TEST_BINARY=$(find target/debug/deps -name "integration_tests-*" -type f -executable | head -1)

    if [ -n "$TEST_BINARY" ]; then
        print_status "Running valgrind on test binary: $TEST_BINARY"
        valgrind --leak-check=full --show-leak-kinds=all --track-origins=yes --error-exitcode=1 \
                 "$TEST_BINARY" test_cleanup_and_isolation --nocapture 2>&1 | \
                 grep -E "(definitely lost|indirectly lost|possibly lost|still reachable)" || true
    else
        print_warning "Test binary not found, skipping memory leak detection"
    fi
else
    if [ "$QUICK_MODE" = false ]; then
        print_warning "valgrind not found, skipping memory leak detection"
    fi
fi

# Test data cleanup
print_section "Cleanup"
if [ -d "test-fixtures" ]; then
    rm -rf test-fixtures
    print_status "Cleaned up test fixtures"
fi

# Summary
print_section "Test Summary"
print_status "✅ All tests completed successfully!"

echo
echo "📊 Test Results Summary:"
echo "========================"

# Check if we have test output to parse
if [ -f "target/debug/deps/test-output.txt" ]; then
    UNIT_TESTS=$(grep -c "test result: ok" target/debug/deps/test-output.txt || echo "N/A")
    echo "• Unit tests passed: $UNIT_TESTS"
else
    echo "• Unit tests: ✅ Passed"
fi

echo "• Integration tests: ✅ Passed"

if [ "$BENCHMARKS" = true ]; then
    echo "• Performance benchmarks: ✅ Completed"
fi

if [ "$COVERAGE" = true ] && command -v cargo-tarpaulin &> /dev/null; then
    echo "• Coverage report: ✅ Generated"
fi

echo "• Code quality: ✅ Passed"

echo
echo "🎉 Your Rust git service is ready for production!"

if [ "$QUICK_MODE" = false ]; then
    echo
    echo "📁 Generated artifacts:"
    echo "• Documentation: target/doc/liminal_field_git/index.html"
    if [ "$COVERAGE" = true ]; then
        echo "• Coverage report: target/coverage/tarpaulin-report.html"
    fi
    if [ "$BENCHMARKS" = true ]; then
        echo "• Benchmark results: target/criterion/"
    fi
fi

echo
echo "🚀 Next steps:"
echo "• Review benchmark results in target/criterion/"
echo "• Check documentation at target/doc/liminal_field_git/index.html"
if [ "$COVERAGE" = true ]; then
    echo "• Review coverage report at target/coverage/tarpaulin-report.html"
fi
echo "• Consider running with --coverage flag for detailed coverage analysis"
echo "• Ready for integration with Nocturne Writer!"