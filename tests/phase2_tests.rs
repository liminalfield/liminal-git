// tests/phase2_tests.rs
use liminal_field_git::{GitService, RepositoryConfig, RepositoryHealth, GitConfig, RepositoryInfo};
use tempfile::TempDir;
use std::fs;

#[cfg(test)]
mod repository_management_tests {
    use super::*;

    #[test]
    fn test_init_repository_success() {
        let temp_dir = TempDir::new().unwrap();
        let service = GitService::new();

        let result = service.init_repository(temp_dir.path().to_string_lossy().to_string());
        assert!(result.is_ok());
        assert!(result.unwrap());

        // Verify .git directory was created
        assert!(temp_dir.path().join(".git").exists());

        // Verify it's a valid repository
        let is_repo = service.is_repository(temp_dir.path().to_string_lossy().to_string());
        assert!(is_repo.is_ok());
        assert!(is_repo.unwrap());
    }

    #[cfg(test)]
    mod phase2_validation_tests {
        use super::*;

        #[test]
        fn test_repository_config_validation() {
            let service = GitService::new();
            let temp_dir = TempDir::new().unwrap();
            service.init_repository(temp_dir.path().to_string_lossy().to_string()).unwrap();

            // Valid config
            let valid_config = RepositoryConfig {
                description: Some("Valid description".to_string()),
                default_branch: Some("main".to_string()),
                line_ending: Some("lf".to_string()),
            };

            let result = service.init_repository_with_config(
                TempDir::new().unwrap().path().to_string_lossy().to_string(),
                valid_config
            );
            assert!(result.is_ok());

            // Invalid line ending
            let invalid_config = RepositoryConfig {
                description: None,
                default_branch: None,
                line_ending: Some("invalid".to_string()),
            };

            let result = service.init_repository_with_config(
                TempDir::new().unwrap().path().to_string_lossy().to_string(),
                invalid_config
            );
            assert!(result.is_err());
        }

        #[test]
        fn test_gitignore_validation() {
            let temp_dir = TempDir::new().unwrap();
            let service = GitService::new();
            service.init_repository(temp_dir.path().to_string_lossy().to_string()).unwrap();

            // Empty patterns should fail
            let result = service.create_gitignore(
                temp_dir.path().to_string_lossy().to_string(),
                vec![]
            );
            assert!(result.is_err());

            // Valid patterns should succeed
            let result = service.create_gitignore(
                temp_dir.path().to_string_lossy().to_string(),
                vec!["*.log".to_string()]
            );
            assert!(result.is_ok());
        }

        #[test]
        fn test_git_config_validation() {
            let temp_dir = TempDir::new().unwrap();
            let service = GitService::new();
            service.init_repository(temp_dir.path().to_string_lossy().to_string()).unwrap();

            // Invalid autocrlf value
            let invalid_config = GitConfig {
                user_name: Some("Test User".to_string()),
                user_email: Some("test@example.com".to_string()),
                core_autocrlf: Some("invalid".to_string()),
                core_safecrlf: None,
            };

            let result = service.configure_repository(
                temp_dir.path().to_string_lossy().to_string(),
                invalid_config
            );
            assert!(result.is_err());
        }
    }
}