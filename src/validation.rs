// validation.rs

use napi::Error as NapiError;
use napi::Status;
use std::path::Path;
use crate::types::{RepositoryConfig, GitConfig};

pub fn validate_repo_path(repo_path: &str) -> Result<(), NapiError> {
    if repo_path.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Repository path cannot be empty"));
    }

    // Check for null bytes (security issue)
    if repo_path.contains('\0') {
        return Err(NapiError::new(Status::InvalidArg, "Repository path contains null bytes"));
    }

    // Check path length (prevent extremely long paths)
    if repo_path.len() > 4096 {
        return Err(NapiError::new(Status::InvalidArg, "Repository path too long"));
    }

    let path = Path::new(repo_path);

    // Check if path exists
    if !path.exists() {
        return Err(NapiError::new(Status::InvalidArg, "Repository path does not exist"));
    }

    // Check if it's a directory
    if !path.is_dir() {
        return Err(NapiError::new(Status::InvalidArg, "Repository path is not a directory"));
    }

    Ok(())
}

pub fn validate_file_path(file_path: &str) -> Result<(), NapiError> {
    if file_path.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "File path cannot be empty"));
    }

    // Check for null bytes
    if file_path.contains('\0') {
        return Err(NapiError::new(Status::InvalidArg, "File path contains null bytes"));
    }

    // Check path length
    if file_path.len() > 4096 {
        return Err(NapiError::new(Status::InvalidArg, "File path too long"));
    }

    // Check for suspicious patterns
    if file_path.contains("..") {
        return Err(NapiError::new(Status::InvalidArg, "Path traversal not allowed"));
    }

    Ok(())
}

pub fn validate_directory_path(dir_path: &str) -> Result<(), NapiError> {
    if dir_path.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Directory path cannot be empty"));
    }

    if dir_path.contains('\0') {
        return Err(NapiError::new(Status::InvalidArg, "Directory path contains null bytes"));
    }

    if dir_path.len() > 4096 {
        return Err(NapiError::new(Status::InvalidArg, "Directory path too long"));
    }

    // Check for suspicious patterns (e.g., path traversal)
    if dir_path.contains("..") {
        return Err(NapiError::new(Status::InvalidArg, "Path traversal not allowed"));
    }

    // Note: We don't check if the directory exists or is a directory here,
    // as the operation (e.g., move) might create it or expect it to be created.
    // This validation is primarily for path format and safety.

    Ok(())
}

pub fn validate_commit_message(message: &str) -> Result<(), NapiError> {
    if message.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Commit message cannot be empty"));
    }

    if message.len() > 10000 {
        return Err(NapiError::new(Status::InvalidArg, "Commit message too long"));
    }

    // Check for null bytes
    if message.contains('\0') {
        return Err(NapiError::new(Status::InvalidArg, "Commit message contains null bytes"));
    }

    Ok(())
}

pub fn validate_user_info(user_name: &str, user_email: &str) -> Result<(), NapiError> {
    if user_name.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "User name cannot be empty"));
    }

    if user_email.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "User email cannot be empty"));
    }

    if user_name.len() > 255 {
        return Err(NapiError::new(Status::InvalidArg, "User name too long"));
    }

    if user_email.len() > 255 {
        return Err(NapiError::new(Status::InvalidArg, "User email too long"));
    }

    // Basic email validation
    if !user_email.contains('@') || !user_email.contains('.') {
        return Err(NapiError::new(Status::InvalidArg, "Invalid email format"));
    }

    // More thorough email validation
    let at_pos = user_email.find('@').unwrap();
    let local_part = &user_email[..at_pos];
    let domain_part = &user_email[at_pos + 1..];

    if local_part.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Invalid email format: missing local part"));
    }
    if domain_part.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Invalid email format: missing domain"));
    }
    if !domain_part.contains('.') {
        return Err(NapiError::new(Status::InvalidArg, "Invalid email format: domain missing dot"));
    }

    // Check for null bytes
    if user_name.contains('\0') || user_email.contains('\0') {
        return Err(NapiError::new(Status::InvalidArg, "User info contains null bytes"));
    }

    Ok(())
}

pub fn validate_file_paths(file_paths: &[String]) -> Result<(), NapiError> {
    if file_paths.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "File paths list cannot be empty"));
    }

    if file_paths.len() > 1000 {
        return Err(NapiError::new(Status::InvalidArg, "Too many files to process"));
    }

    for file_path in file_paths {
        validate_file_path(file_path)?;
    }

    Ok(())
}


pub fn validate_repository_config(config: &RepositoryConfig) -> Result<(), NapiError> {
    if let Some(ref description) = config.description {
        if description.len() > 1000 {
            return Err(NapiError::new(Status::InvalidArg, "Repository description too long"));
        }
        if description.contains('\0') {
            return Err(NapiError::new(Status::InvalidArg, "Repository description contains null bytes"));
        }
    }

    if let Some(ref branch) = config.default_branch {
        if branch.is_empty() {
            return Err(NapiError::new(Status::InvalidArg, "Default branch name cannot be empty"));
        }
        if branch.len() > 255 {
            return Err(NapiError::new(Status::InvalidArg, "Default branch name too long"));
        }
        if branch.contains('\0') || branch.contains(' ') || branch.starts_with('-') {
            return Err(NapiError::new(Status::InvalidArg, "Invalid branch name format"));
        }
    }

    if let Some(ref line_ending) = config.line_ending {
        match line_ending.as_str() {
            "lf" | "crlf" | "auto" => {},
            _ => return Err(NapiError::new(Status::InvalidArg, "Invalid line ending option. Must be 'lf', 'crlf', or 'auto'")),
        }
    }

    Ok(())
}

pub fn validate_git_config(config: &GitConfig) -> Result<(), NapiError> {
    if let Some(ref user_name) = config.user_name {
        validate_user_info(user_name, "dummy@example.com")?;
    }

    if let Some(ref user_email) = config.user_email {
        validate_user_info("Dummy User", user_email)?;
    }

    if let Some(ref autocrlf) = config.core_autocrlf {
        match autocrlf.as_str() {
            "true" | "false" | "input" => {},
            _ => return Err(NapiError::new(Status::InvalidArg, "Invalid core.autocrlf value. Must be 'true', 'false', or 'input'")),
        }
    }

    if let Some(ref safecrlf) = config.core_safecrlf {
        match safecrlf.as_str() {
            "true" | "false" | "warn" => {},
            _ => return Err(NapiError::new(Status::InvalidArg, "Invalid core.safecrlf value. Must be 'true', 'false', or 'warn'")),
        }
    }

    Ok(())
}

pub fn validate_gitignore_patterns(patterns: &[String]) -> Result<(), NapiError> {
    if patterns.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Gitignore patterns cannot be empty"));
    }

    if patterns.len() > 1000 {
        return Err(NapiError::new(Status::InvalidArg, "Too many gitignore patterns"));
    }

    for pattern in patterns {
        if pattern.len() > 4096 {
            return Err(NapiError::new(Status::InvalidArg, "Gitignore pattern too long"));
        }
        if pattern.contains('\0') {
            return Err(NapiError::new(Status::InvalidArg, "Gitignore pattern contains null bytes"));
        }
    }

    Ok(())
}

pub fn validate_gitattributes_rules(rules: &[String]) -> Result<(), NapiError> {
    if rules.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Gitattributes rules cannot be empty"));
    }

    if rules.len() > 1000 {
        return Err(NapiError::new(Status::InvalidArg, "Too many gitattributes rules"));
    }

    for rule in rules {
        if rule.len() > 4096 {
            return Err(NapiError::new(Status::InvalidArg, "Gitattributes rule too long"));
        }
        if rule.contains('\0') {
            return Err(NapiError::new(Status::InvalidArg, "Gitattributes rule contains null bytes"));
        }
    }

    Ok(())
}

pub fn validate_directory_for_init(path: &str) -> Result<(), NapiError> {
    validate_repo_path(path)?;

    let path_buf = Path::new(path);

    // Check if directory exists
    if !path_buf.exists() {
        return Err(NapiError::new(Status::InvalidArg, "Directory does not exist"));
    }

    // Check if it's already a git repository
    if path_buf.join(".git").exists() {
        return Err(NapiError::new(Status::InvalidArg, "Directory is already a git repository"));
    }

    Ok(())
}


pub fn validate_commit_hash(commit_hash: &str) -> Result<(), NapiError> {
    if commit_hash.is_empty() {
        return Err(NapiError::new(Status::InvalidArg, "Commit hash cannot be empty"));
    }

    if commit_hash.len() < 4 || commit_hash.len() > 40 {
        return Err(NapiError::new(Status::InvalidArg, "Invalid commit hash length"));
    }

    if !commit_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(NapiError::new(Status::InvalidArg, "Commit hash must contain only hexadecimal characters"));
    }

    Ok(())
}

pub fn validate_history_pagination(limit: Option<usize>, offset: Option<usize>) -> Result<(), NapiError> {
    if let Some(limit) = limit {
        if limit == 0 {
            return Err(NapiError::new(Status::InvalidArg, "Limit must be greater than 0"));
        }
        if limit > 1000 {
            return Err(NapiError::new(Status::InvalidArg, "Limit cannot exceed 1000"));
        }
    }

    if let Some(offset) = offset {
        if offset > 100000 {
            return Err(NapiError::new(Status::InvalidArg, "Offset too large"));
        }
    }

    Ok(())
}

pub fn validate_file_path_for_history(file_path: &str) -> Result<(), NapiError> {
    validate_file_path(file_path)?;

    // Additional validation for history operations
    if file_path.starts_with('/') {
        return Err(NapiError::new(Status::InvalidArg, "File path should be relative to repository root"));
    }

    Ok(())
}

pub fn validate_restore_operation(repo_path: &str, file_path: &str, commit_hash: &str) -> Result<(), NapiError> {
    validate_repo_path(repo_path)?;
    validate_file_path_for_history(file_path)?;
    validate_commit_hash(commit_hash)?;

    // Check if target file would overwrite existing work
    let full_path = Path::new(repo_path).join(file_path);
    if full_path.exists() {
        // This is a warning case - we'll allow it but the caller should confirm
        // For now, we'll allow the operation
    }

    Ok(())
}

pub fn validate_diff_parameters(repo_path: &str, file_path: Option<&str>) -> Result<(), NapiError> {
    validate_repo_path(repo_path)?;

    if let Some(path) = file_path {
        validate_file_path_for_history(path)?;
    }

    Ok(())
}

pub fn validate_deleted_files_limit(limit: Option<usize>) -> Result<(), NapiError> {
    if let Some(limit) = limit {
        if limit == 0 {
            return Err(NapiError::new(Status::InvalidArg, "Limit must be greater than 0"));
        }
        if limit > 500 {
            return Err(NapiError::new(Status::InvalidArg, "Limit for deleted files cannot exceed 500"));
        }
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_validate_repo_path_valid() {
        let temp_dir = TempDir::new().unwrap();
        let result = validate_repo_path(temp_dir.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_repo_path_empty() {
        let result = validate_repo_path("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_repo_path_null_bytes() {
        let result = validate_repo_path("path\0with\0nulls");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_validate_repo_path_too_long() {
        let long_path = "a".repeat(4097);
        let result = validate_repo_path(&long_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_validate_repo_path_nonexistent() {
        let result = validate_repo_path("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_validate_repo_path_not_directory() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        fs::write(&file_path, "content").unwrap();

        let result = validate_repo_path(file_path.to_str().unwrap());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }

    #[test]
    fn test_validate_file_path_valid() {
        let valid_paths = vec![
            "README.md",
            "src/main.rs",
            "docs/guide.md",
            "tests/test_file.rs",
            "deep/nested/path/file.txt",
        ];

        for path in valid_paths {
            let result = validate_file_path(path);
            assert!(result.is_ok(), "Path should be valid: {}", path);
        }
    }

    #[test]
    fn test_validate_file_path_empty() {
        let result = validate_file_path("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_file_path_null_bytes() {
        let result = validate_file_path("file\0with\0nulls.txt");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_validate_file_path_too_long() {
        let long_path = "a".repeat(4097);
        let result = validate_file_path(&long_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_validate_file_path_traversal_attempts() {
        let traversal_attempts = vec![
            "../outside.txt",
            "subdir/../../../etc/passwd",
            "..\\windows\\path",
            "normal/../traversal/file.txt",
        ];

        for path in traversal_attempts {
            let result = validate_file_path(path);
            assert!(result.is_err(), "Path traversal should be rejected: {}", path);
            let error_msg = result.unwrap_err().to_string();
            assert!(error_msg.contains("traversal"));
        }
    }

    #[test]
    fn test_validate_commit_message_valid() {
        let valid_messages = vec![
            "Simple commit message",
            "feat: add new feature",
            "fix: resolve bug #123",
            "docs: update README.md",
            "Multi-line\ncommit\nmessage",
            "Message with unicode: 🦀",
        ];

        for message in valid_messages {
            let result = validate_commit_message(message);
            assert!(result.is_ok(), "Message should be valid: {}", message);
        }
    }

    #[test]
    fn test_validate_commit_message_empty() {
        let result = validate_commit_message("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_commit_message_too_long() {
        let long_message = "a".repeat(10001);
        let result = validate_commit_message(&long_message);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_validate_commit_message_null_bytes() {
        let result = validate_commit_message("message\0with\0nulls");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_validate_user_info_valid() {
        let valid_users = vec![
            ("John Doe", "john@example.com"),
            ("Jane Smith", "jane.smith@company.org"),
            ("Test User", "test+tag@domain.co.uk"),
            ("Unicode Name 🦀", "unicode@example.com"),
        ];

        for (name, email) in valid_users {
            let result = validate_user_info(name, email);
            assert!(result.is_ok(), "User info should be valid: {} <{}>", name, email);
        }
    }

    #[test]
    fn test_validate_user_info_empty_name() {
        let result = validate_user_info("", "test@example.com");
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("name"));
        assert!(error_msg.contains("empty"));
    }

    #[test]
    fn test_validate_user_info_empty_email() {
        let result = validate_user_info("Test User", "");
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("email"));
        assert!(error_msg.contains("empty"));
    }

    #[test]
    fn test_validate_user_info_invalid_email() {
        let invalid_emails = vec![
            "invalid-email",
            "no-at-symbol.com",
            "no-domain@",
            "@no-local-part.com",
            "missing.dot@domain",
        ];

        for email in invalid_emails {
            let result = validate_user_info("Test User", email);
            assert!(result.is_err(), "Email should be invalid: {}", email);
            let error_msg = result.unwrap_err().to_string();
            assert!(error_msg.contains("email"));
        }
    }

    #[test]
    fn test_validate_user_info_too_long() {
        let long_name = "a".repeat(256);
        let long_email = format!("{}@example.com", "a".repeat(250));

        let result = validate_user_info(&long_name, "test@example.com");
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("name"));
        assert!(error_msg.contains("too long"));

        let result = validate_user_info("Test User", &long_email);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("email"));
        assert!(error_msg.contains("too long"));
    }

    #[test]
    fn test_validate_user_info_null_bytes() {
        let result = validate_user_info("User\0Name", "test@example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));

        let result = validate_user_info("Test User", "test\0@example.com");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("null bytes"));
    }

    #[test]
    fn test_validate_file_paths_valid() {
        let valid_paths = vec![
            "README.md".to_string(),
            "src/main.rs".to_string(),
            "tests/test.rs".to_string(),
        ];

        let result = validate_file_paths(&valid_paths);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_paths_empty() {
        let result = validate_file_paths(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_validate_file_paths_too_many() {
        let many_paths: Vec<String> = (0..1001)
            .map(|i| format!("file_{}.txt", i))
            .collect();

        let result = validate_file_paths(&many_paths);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Too many"));
    }

    #[test]
    fn test_validate_file_paths_invalid_path() {
        let paths_with_invalid = vec![
            "valid.txt".to_string(),
            "../invalid.txt".to_string(),
            "also_valid.txt".to_string(),
        ];

        let result = validate_file_paths(&paths_with_invalid);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_edge_case_inputs() {
        let edge_cases = vec![
            " ",
            "\n",
            "\t",
            "file with spaces.txt",
            "file-with-unicode-🦀.rs",
        ];

        for input in edge_cases {
            // These should not panic, though they may return errors
            let _path_result = validate_file_path(input);
            let _message_result = validate_commit_message(input);
            let _user_result = validate_user_info(input, "test@example.com");
        }
    }

    #[test]
    fn test_concurrent_validation() {
        use std::thread;
        use std::sync::Arc;

        let temp_dir = Arc::new(TempDir::new().unwrap());
        let mut handles = vec![];

        for i in 0..10 {
            let _temp_dir = Arc::clone(&temp_dir);
            let handle = thread::spawn(move || {
                let path = format!("test_file_{}.txt", i);
                validate_file_path(&path)
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.join().unwrap();
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_boundary_conditions() {
        // Test exactly at the boundaries
        let path_4096 = "a".repeat(4096);
        assert!(validate_repo_path(&path_4096).is_err()); // Should fail because path doesn't exist

        let path_4095 = "a".repeat(4095);
        assert!(validate_file_path(&path_4095).is_ok());

        let message_10000 = "a".repeat(10000);
        assert!(validate_commit_message(&message_10000).is_ok());

        let message_10001 = "a".repeat(10001);
        assert!(validate_commit_message(&message_10001).is_err());

        let name_255 = "a".repeat(255);
        assert!(validate_user_info(&name_255, "test@example.com").is_ok());

        let name_256 = "a".repeat(256);
        assert!(validate_user_info(&name_256, "test@example.com").is_err());
    }
}

#[cfg(test)]
mod phase2_validation_tests {
    use super::*;
    use crate::types::{RepositoryConfig, GitConfig};

    #[test]
    fn test_validate_repository_config_valid() {
        let config = RepositoryConfig {
            description: Some("Valid description".to_string()),
            default_branch: Some("main".to_string()),
            line_ending: Some("lf".to_string()),
        };

        let result = validate_repository_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_repository_config_long_description() {
        let config = RepositoryConfig {
            description: Some("x".repeat(1001)),
            default_branch: None,
            line_ending: None,
        };

        let result = validate_repository_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_repository_config_invalid_line_ending() {
        let config = RepositoryConfig {
            description: None,
            default_branch: None,
            line_ending: Some("invalid".to_string()),
        };

        let result = validate_repository_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_repository_config_invalid_branch_name() {
        let config = RepositoryConfig {
            description: None,
            default_branch: Some("-invalid".to_string()),
            line_ending: None,
        };

        let result = validate_repository_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_git_config_valid() {
        let config = GitConfig {
            user_name: Some("Test User".to_string()),
            user_email: Some("test@example.com".to_string()),
            core_autocrlf: Some("false".to_string()),
            core_safecrlf: Some("warn".to_string()),
        };

        let result = validate_git_config(&config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_git_config_invalid_autocrlf() {
        let config = GitConfig {
            user_name: None,
            user_email: None,
            core_autocrlf: Some("invalid".to_string()),
            core_safecrlf: None,
        };

        let result = validate_git_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_gitignore_patterns_valid() {
        let patterns = vec!["*.log".to_string(), "node_modules/".to_string()];
        let result = validate_gitignore_patterns(&patterns);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_gitignore_patterns_empty() {
        let patterns = vec![];
        let result = validate_gitignore_patterns(&patterns);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_gitignore_patterns_too_many() {
        let patterns = vec!["pattern".to_string(); 1001];
        let result = validate_gitignore_patterns(&patterns);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_gitattributes_rules_valid() {
        let rules = vec!["*.txt text".to_string(), "*.jpg binary".to_string()];
        let result = validate_gitattributes_rules(&rules);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_gitattributes_rules_empty() {
        let rules = vec![];
        let result = validate_gitattributes_rules(&rules);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_directory_for_init_nonexistent() {
        let result = validate_directory_for_init("/nonexistent/path");
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod phase3_validation_tests {
    use super::*;

    #[test]
    fn test_validate_commit_hash_valid() {
        // Valid full hash
        let result = validate_commit_hash("1234567890abcdef1234567890abcdef12345678");
        assert!(result.is_ok());

        // Valid short hash
        let result = validate_commit_hash("1234567");
        assert!(result.is_ok());

        // Valid minimal hash
        let result = validate_commit_hash("1234");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_commit_hash_invalid() {
        // Empty hash
        let result = validate_commit_hash("");
        assert!(result.is_err());

        // Too short
        let result = validate_commit_hash("123");
        assert!(result.is_err());

        // Too long
        let result = validate_commit_hash("1234567890abcdef1234567890abcdef123456789");
        assert!(result.is_err());

        // Invalid characters
        let result = validate_commit_hash("12345xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_history_pagination_valid() {
        let result = validate_history_pagination(Some(50), Some(0));
        assert!(result.is_ok());

        let result = validate_history_pagination(Some(1), Some(100));
        assert!(result.is_ok());

        let result = validate_history_pagination(None, None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_history_pagination_invalid() {
        // Zero limit
        let result = validate_history_pagination(Some(0), None);
        assert!(result.is_err());

        // Limit too large
        let result = validate_history_pagination(Some(1001), None);
        assert!(result.is_err());

        // Offset too large
        let result = validate_history_pagination(Some(50), Some(100001));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_file_path_for_history_valid() {
        let result = validate_file_path_for_history("file.txt");
        assert!(result.is_ok());

        let result = validate_file_path_for_history("folder/file.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_file_path_for_history_invalid() {
        // Absolute path
        let result = validate_file_path_for_history("/absolute/path.txt");
        assert!(result.is_err());

        // Empty path
        let result = validate_file_path_for_history("");
        assert!(result.is_err());

        // Path with null bytes
        let result = validate_file_path_for_history("file\0.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_restore_operation_with_invalid_inputs() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_string_lossy().to_string();

        // Test with invalid commit hash
        let result = validate_restore_operation(&repo_path, "file.txt", "xyz");
        assert!(result.is_err());

        // Test with invalid file path
        let result = validate_restore_operation(&repo_path, "", "1234567");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_diff_parameters_with_invalid_inputs() {
        // Test with invalid file path
        let result = validate_diff_parameters("/valid/path", Some(""));
        assert!(result.is_err());

        // Test with invalid repo path
        let result = validate_diff_parameters("", Some("file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_deleted_files_limit_valid() {
        let result = validate_deleted_files_limit(Some(100));
        assert!(result.is_ok());

        let result = validate_deleted_files_limit(Some(1));
        assert!(result.is_ok());

        let result = validate_deleted_files_limit(None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_deleted_files_limit_invalid() {
        // Zero limit
        let result = validate_deleted_files_limit(Some(0));
        assert!(result.is_err());

        // Limit too large
        let result = validate_deleted_files_limit(Some(501));
        assert!(result.is_err());
    }
}