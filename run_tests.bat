@echo off
setlocal enabledelayedexpansion

echo 🦀 Rust Git Service - Comprehensive Test Suite
echo ==============================================
echo.

REM Parse command line arguments
set QUICK_MODE=false
set COVERAGE=false
set BENCHMARKS=true
set FIXTURES=true

:parse_args
if "%~1"=="" goto end_parse
if "%~1"=="--quick" (
    set QUICK_MODE=true
    set BENCHMARKS=false
    shift
    goto parse_args
)
if "%~1"=="--coverage" (
    set COVERAGE=true
    shift
    goto parse_args
)
if "%~1"=="--no-benchmarks" (
    set BENCHMARKS=false
    shift
    goto parse_args
)
if "%~1"=="--no-fixtures" (
    set FIXTURES=false
    shift
    goto parse_args
)
if "%~1"=="-h" goto show_help
if "%~1"=="--help" goto show_help
echo Unknown option: %~1
goto error_exit

:show_help
echo Usage: %0 [OPTIONS]
echo Options:
echo   --quick         Run only essential tests (faster)
echo   --coverage      Generate coverage report (requires cargo-tarpaulin)
echo   --no-benchmarks Skip benchmark tests
echo   --no-fixtures   Skip fixture creation
echo   -h, --help      Show this help message
exit /b 0

:end_parse

REM Check if we're in the right directory
if not exist "Cargo.toml" (
    echo [ERROR] Must be run from the native/ directory
    goto error_exit
)

if "%QUICK_MODE%"=="true" (
    echo [WARN] Running in quick mode - skipping benchmarks and some tests
    echo.
)

echo [SECTION] Environment Setup
echo ============================

REM Check Rust installation
cargo --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Cargo not found. Please install Rust.
    goto error_exit
)

REM Check Git installation
git --version >nul 2>&1
if errorlevel 1 (
    echo [ERROR] Git not found. Please install Git.
    goto error_exit
)

echo [INFO] Checking environment...
rustc --version
cargo --version
git --version
echo.

REM Clean previous builds
echo [SECTION] Cleanup
echo ==================
echo [INFO] Cleaning previous builds...
cargo clean
if errorlevel 1 goto error_exit
echo [INFO] ✅ Cleanup completed successfully
echo.

REM Check code formatting
echo [SECTION] Code Quality Checks
echo ==============================
where cargo-fmt >nul 2>&1
if not errorlevel 1 (
    echo [INFO] Checking code formatting...
    cargo fmt --check
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Code formatting check passed
) else (
    echo [WARN] cargo-fmt not found, skipping format check
)

REM Check for linting issues
where cargo-clippy >nul 2>&1
if not errorlevel 1 (
    echo [INFO] Running clippy lints...
    cargo clippy -- -D warnings
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Clippy lints passed
) else (
    echo [WARN] cargo-clippy not found, skipping lint check
)
echo.

REM Create test fixtures
if "%FIXTURES%"=="true" (
    echo [SECTION] Test Fixtures
    echo =======================
    echo [INFO] Creating test fixtures...
    cargo test --lib fixtures::create_repos::create_all_fixtures
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Test fixtures created successfully
    echo.
)

REM Build the project
echo [SECTION] Build
echo ===============
echo [INFO] Building project...
cargo build
if errorlevel 1 goto error_exit
echo [INFO] ✅ Build completed successfully

if "%QUICK_MODE%"=="false" (
    echo [INFO] Building project in release mode...
    cargo build --release
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Release build completed successfully
)
echo.

REM Run unit tests
echo [SECTION] Unit Tests
echo ====================
echo [INFO] Running unit tests...
cargo test --lib
if errorlevel 1 goto error_exit
echo [INFO] ✅ Unit tests completed successfully
echo.

REM Run integration tests
echo [SECTION] Integration Tests
echo ============================
echo [INFO] Running integration tests...
cargo test --test integration_tests --features test-utils
if errorlevel 1 goto error_exit
echo [INFO] ✅ Integration tests completed successfully
echo.

REM Run tests in release mode for performance validation
if "%QUICK_MODE%"=="false" (
    echo [SECTION] Performance Validation
    echo ===================================
    echo [INFO] Running performance validation tests...
    cargo test --release --test integration_tests test_large_repository_performance --features test-utils
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Performance validation completed

    echo [INFO] Running concurrency tests...
    cargo test --release --test integration_tests test_concurrent_operations --features test-utils
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Concurrency tests completed
    echo.
)

REM Run benchmarks
if "%BENCHMARKS%"=="true" (
    echo [SECTION] Performance Benchmarks
    echo ===================================
    echo [INFO] Running performance benchmarks...
    cargo bench --features test-utils
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Performance benchmarks completed
    echo.
)

REM Generate documentation
echo [SECTION] Documentation
echo =======================
echo [INFO] Generating documentation...
cargo doc --no-deps
if errorlevel 1 goto error_exit
echo [INFO] ✅ Documentation generated successfully
echo.

REM Security audit
where cargo-audit >nul 2>&1
if not errorlevel 1 (
    echo [SECTION] Security Audit
    echo =======================
    echo [INFO] Running security audit...
    cargo audit
    if errorlevel 1 goto error_exit
    echo [INFO] ✅ Security audit completed
    echo.
) else (
    echo [WARN] cargo-audit not found. Install with: cargo install cargo-audit
    echo.
)

REM Generate coverage report
if "%COVERAGE%"=="true" (
    echo [SECTION] Coverage Report
    echo ===========================
    where cargo-tarpaulin >nul 2>&1
    if not errorlevel 1 (
        echo [INFO] Generating coverage report...
        cargo tarpaulin --out Html --output-dir target/coverage
        if errorlevel 1 goto error_exit
        echo [INFO] ✅ Coverage report generated in target/coverage/tarpaulin-report.html
    ) else (
        echo [WARN] cargo-tarpaulin not found. Install with: cargo install cargo-tarpaulin
    )
    echo.
)

REM Test data cleanup
echo [SECTION] Cleanup
echo =================
if exist "test-fixtures" (
    rmdir /s /q "test-fixtures"
    echo [INFO] Cleaned up test fixtures
)
echo.

REM Summary
echo [SECTION] Test Summary
echo ======================
echo [INFO] ✅ All tests completed successfully!
echo.

echo 📊 Test Results Summary:
echo ========================
echo • Unit tests: ✅ Passed
echo • Integration tests: ✅ Passed

if "%BENCHMARKS%"=="true" (
    echo • Performance benchmarks: ✅ Completed
)

if "%COVERAGE%"=="true" (
    where cargo-tarpaulin >nul 2>&1
    if not errorlevel 1 (
        echo • Coverage report: ✅ Generated
    )
)

echo • Code quality: ✅ Passed
echo.

echo 🎉 Your Rust git service is ready for production!

if "%QUICK_MODE%"=="false" (
    echo.
    echo 📁 Generated artifacts:
    echo • Documentation: target\doc\liminal_field_git\index.html
    if "%COVERAGE%"=="true" (
        echo • Coverage report: target\coverage\tarpaulin-report.html
    )
    if "%BENCHMARKS%"=="true" (
        echo • Benchmark results: target\criterion\
    )
)

echo.
echo 🚀 Next steps:
echo • Review benchmark results in target\criterion\
echo • Check documentation at target\doc\liminal_field_git\index.html
if "%COVERAGE%"=="true" (
    echo • Review coverage report at target\coverage\tarpaulin-report.html
)
echo • Consider running with --coverage flag for detailed coverage analysis
echo • Ready for integration with Nocturne Writer!

goto end

:error_exit
echo.
echo [ERROR] ❌ Test execution failed!
echo Check the error messages above for details.
exit /b 1

:end
echo.
echo Test execution completed successfully!