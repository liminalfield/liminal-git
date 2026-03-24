use tempfile::TempDir;
use serial_test::serial;

// Import the git service modules - note: hyphens become underscores
use liminal_field_git::{GitService, test_utils};

mod fixtures;
use fixtures::create_repos;

// Helper function to create a test git service
fn create_git_service() -> GitService {
    GitService::new()
}

#[serial]
#[test]
fn test_complete_git_workflow() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // 1. Verify repository detection
    let is_repo = git_service.is_repository(repo_path.clone()).unwrap();
    assert!(is_repo);

    // 2. Check initial status (should be clean)
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert_eq!(status.modified_files.len(), 0);
    assert_eq!(status.untracked_files.len(), 0);
    assert_eq!(status.staged_files.len(), 0);
    assert!(status.is_clean);

    // 3. Add some files
    test_repo.add_file("README.md", "# Test Repository").unwrap();
    test_repo.add_file("src/main.rs", "fn main() {}").unwrap();

    // 4. Check status after adding files
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert_eq!(status.untracked_files.len(), 2);
    assert!(!status.is_clean);

    // 5. Stage files
    git_service.stage_file(repo_path.clone(), "README.md".to_string()).unwrap();
    git_service.stage_file(repo_path.clone(), "src/main.rs".to_string()).unwrap();

    // 6. Check status after staging
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert_eq!(status.staged_files.len(), 2);
    assert_eq!(status.untracked_files.len(), 0);

    // 7. Commit staged files
    let commit_hash = git_service.commit_files(
        repo_path.clone(),
        vec!["README.md".to_string(), "src/main.rs".to_string()],
        "Initial commit".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    ).unwrap();

    assert!(!commit_hash.is_empty());
    assert_eq!(commit_hash.len(), 40); // SHA-1 hash length

    // 8. Verify clean status after commit
    let status = git_service.get_status(repo_path).unwrap();
    assert_eq!(status.modified_files.len(), 0);
    assert_eq!(status.untracked_files.len(), 0);
    assert_eq!(status.staged_files.len(), 0);
    assert!(status.is_clean);
}

#[serial]
#[test]
fn test_file_modification_workflow() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create and commit initial file
    test_repo.add_and_commit("test.txt", "initial content", "Initial commit").unwrap();

    // Modify the file
    test_repo.add_file("test.txt", "modified content").unwrap();

    // Check status shows modification
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert_eq!(status.modified_files.len(), 1);
    assert_eq!(status.modified_files[0].path, "test.txt");

    // Stage and commit modification
    git_service.stage_file(repo_path.clone(), "test.txt".to_string()).unwrap();

    let commit_hash = git_service.commit_file(
        repo_path.clone(),
        "test.txt".to_string(),
        "Update test.txt".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    ).unwrap();

    assert!(!commit_hash.is_empty());

    // Verify repository is clean
    let status = git_service.get_status(repo_path).unwrap();
    assert!(status.is_clean);
}

#[serial]
#[test]
fn test_mixed_file_states() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create initial commit
    test_repo.add_and_commit("existing.txt", "content", "Initial commit").unwrap();

    // Create files in different states
    test_repo.add_file("existing.txt", "modified content").unwrap(); // Modified
    test_repo.add_file("untracked.txt", "untracked content").unwrap(); // Untracked
    test_repo.add_file("staged.txt", "staged content").unwrap(); // Will be staged

    // Stage one file
    git_service.stage_file(repo_path.clone(), "staged.txt".to_string()).unwrap();

    // Check mixed status
    let status = git_service.get_status(repo_path).unwrap();

    // Should have files in different states
    assert_eq!(status.modified_files.len(), 1);
    assert_eq!(status.untracked_files.len(), 1);
    assert_eq!(status.staged_files.len(), 1);

    assert_eq!(status.modified_files[0].path, "existing.txt");
    assert_eq!(status.untracked_files[0].path, "untracked.txt");
    assert_eq!(status.staged_files[0].path, "staged.txt");
}

#[serial]
#[test]
fn test_error_handling() {
    let git_service = create_git_service();

    // Test operations on non-existent repository
    let result = git_service.get_status("/nonexistent/path".to_string());
    assert!(result.is_err());

    // Test operations on non-git directory
    let temp_dir = TempDir::new().unwrap();
    let result = git_service.get_status(temp_dir.path().to_str().unwrap().to_string());
    assert!(result.is_err());

    // Test invalid file paths
    let test_repo = test_utils::TestRepo::new().unwrap();
    let result = git_service.stage_file(
        test_repo.path_str().to_string(),
        "../outside.txt".to_string()
    );
    assert!(result.is_err());

    // Test invalid commit info
    test_repo.add_file("test.txt", "content").unwrap();
    let result = git_service.commit_file(
        test_repo.path_str().to_string(),
        "test.txt".to_string(),
        "".to_string(), // Empty message
        "Test".to_string(),
        "invalid-email".to_string(), // Invalid email
    );
    assert!(result.is_err());
}

#[serial]
#[test]
fn test_large_repository_performance() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create many files
    for i in 0..100 {
        test_repo.add_file(&format!("file_{}.txt", i), &format!("content {}", i)).unwrap();
    }

    // Time the status operation
    let start = std::time::Instant::now();
    let status = git_service.get_status(repo_path.clone()).unwrap();
    let duration = start.elapsed();

    // Should complete within reasonable time
    assert!(duration < std::time::Duration::from_millis(500));
    assert_eq!(status.untracked_files.len(), 100);

    // Time staging all files
    let file_names: Vec<String> = (0..100).map(|i| format!("file_{}.txt", i)).collect();

    let start = std::time::Instant::now();
    for file_name in &file_names {
        git_service.stage_file(repo_path.clone(), file_name.clone()).unwrap();
    }
    let duration = start.elapsed();

    assert!(duration < std::time::Duration::from_millis(2000));

    // Time committing all files
    let start = std::time::Instant::now();
    let commit_hash = git_service.commit_files(
        repo_path.clone(),
        file_names,
        "Add many files".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    ).unwrap();
    let duration = start.elapsed();

    assert!(duration < std::time::Duration::from_millis(1000));
    assert!(!commit_hash.is_empty());
}

#[serial]
#[test]
fn test_concurrent_operations() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    // Create separate repos for each thread to avoid sharing git2::Repository
    let repo_paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];

    // Spawn multiple threads performing git operations
    for i in 0..5 {
        let paths = Arc::clone(&repo_paths);
        let handle = thread::spawn(move || {
            let test_repo = test_utils::TestRepo::new().unwrap();
            let git_service = create_git_service();
            let file_name = format!("concurrent_{}.txt", i);
            test_repo.add_file(&file_name, &format!("content {}", i)).unwrap();

            // Store the repo path for later verification
            {
                let mut paths_guard = paths.lock().unwrap();
                paths_guard.push(test_repo.path_str().to_string());
            }

            // Each task performs its own status check
            git_service.get_status(test_repo.path_str().to_string())
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result.is_ok());
    }

    // Verify each repo has one untracked file
    let paths = repo_paths.lock().unwrap();
    assert_eq!(paths.len(), 5);

    let git_service = create_git_service();
    for path in paths.iter() {
        let status = git_service.get_status(path.clone()).unwrap();
        assert_eq!(status.untracked_files.len(), 1);
    }
}

#[serial]
#[test]
fn test_repository_with_complex_history() {
    // Use the complex fixture
    create_repos::create_all_fixtures().unwrap();
    let complex_repo = liminal_field_git::test_utils::TestRepo::from_fixture("complex-repo").unwrap();
    let repo_path = complex_repo.path_str().to_string();
    let git_service = create_git_service();

    // Test status on complex repository
    let status = git_service.get_status(repo_path.clone()).unwrap();

    // Should detect the various file states created by the fixture
    assert!(status.modified_files.len() > 0, "Should have modified files");
    assert!(status.staged_files.len() > 0, "Should have staged files");
    assert!(status.untracked_files.len() > 0, "Should have untracked files");

    // Repository should be detected as a valid git repo
    assert!(git_service.is_repository(repo_path).unwrap());
}

#[serial]
#[test]
fn test_binary_file_handling() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Add binary file
    use std::fs;
    let binary_path = test_repo.path.join("binary.dat");
    fs::write(&binary_path, &[0x00, 0x01, 0x02, 0xFF, 0xFE]).unwrap();

    // Should detect as untracked
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert!(status.untracked_files.iter().any(|f| f.path == "binary.dat"));

    // Should be able to stage and commit
    git_service.stage_file(repo_path.clone(), "binary.dat".to_string()).unwrap();

    let result = git_service.commit_file(
        repo_path.clone(),
        "binary.dat".to_string(),
        "Add binary file".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    );

    assert!(result.is_ok());

    // Verify repository is clean
    let status = git_service.get_status(repo_path).unwrap();
    assert!(status.is_clean);
}

#[serial]
#[test]
fn test_cleanup_and_isolation() {
    // Each test should start with a clean state
    let test_repo1 = liminal_field_git::test_utils::TestRepo::new().unwrap();
    let test_repo2 = liminal_field_git::test_utils::TestRepo::new().unwrap();
    let git_service = create_git_service();

    // Repos should be independent
    assert_ne!(test_repo1.path_str(), test_repo2.path_str());

    // Operations on one shouldn't affect the other
    test_repo1.add_file("test1.txt", "content1").unwrap();
    test_repo2.add_file("test2.txt", "content2").unwrap();

    let status1 = git_service.get_status(test_repo1.path_str().to_string()).unwrap();
    let status2 = git_service.get_status(test_repo2.path_str().to_string()).unwrap();

    assert!(status1.untracked_files.iter().any(|f| f.path == "test1.txt"));
    assert!(!status1.untracked_files.iter().any(|f| f.path == "test2.txt"));

    assert!(status2.untracked_files.iter().any(|f| f.path == "test2.txt"));
    assert!(!status2.untracked_files.iter().any(|f| f.path == "test1.txt"));

    // Cleanup is automatic when TestRepo is dropped
}

#[serial]
#[test]
fn test_unstage_operations() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create initial commit
    test_repo.add_and_commit("test.txt", "initial content", "Initial commit").unwrap();

    // Modify and stage the file
    test_repo.add_file("test.txt", "modified content").unwrap();
    git_service.stage_file(repo_path.clone(), "test.txt".to_string()).unwrap();

    // Verify it's staged
    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert_eq!(status.staged_files.len(), 1);

    // Unstage it
    git_service.unstage_file(repo_path.clone(), "test.txt".to_string()).unwrap();

    // Verify it's no longer staged but still modified
    let status = git_service.get_status(repo_path).unwrap();
    assert_eq!(status.staged_files.len(), 0);
    assert_eq!(status.modified_files.len(), 1);
}

#[serial]
#[test]
fn test_get_staged_files() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Initially should have no staged files
    let staged_files = git_service.get_staged_files(repo_path.clone()).unwrap();
    assert_eq!(staged_files.len(), 0);

    // Add and stage multiple files
    test_repo.add_file("file1.txt", "content1").unwrap();
    test_repo.add_file("file2.txt", "content2").unwrap();
    git_service.stage_file(repo_path.clone(), "file1.txt".to_string()).unwrap();
    git_service.stage_file(repo_path.clone(), "file2.txt".to_string()).unwrap();

    // Should now return the staged files
    let staged_files = git_service.get_staged_files(repo_path).unwrap();
    assert_eq!(staged_files.len(), 2);
    assert!(staged_files.contains(&"file1.txt".to_string()));
    assert!(staged_files.contains(&"file2.txt".to_string()));
}

#[serial]
#[test]
fn test_unicode_and_special_characters() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create files with unicode names and content
    test_repo.add_file("测试.txt", "Chinese content: 你好世界").unwrap();
    test_repo.add_file("файл.txt", "Russian content: Привет мир").unwrap();
    test_repo.add_file("🦀_rust.rs", "Rust with emoji: fn main() {}").unwrap();

    let files = vec![
        "测试.txt".to_string(),
        "файл.txt".to_string(),
        "🦀_rust.rs".to_string()
    ];

    // Stage all files
    for file in &files {
        git_service.stage_file(repo_path.clone(), file.clone()).unwrap();
    }

    // Commit with unicode commit message
    let result = git_service.commit_files(
        repo_path.clone(),
        files,
        "添加国际化文件 🌍".to_string(), // Unicode commit message
        "测试用户".to_string(),          // Unicode name
        "test@example.com".to_string(),
    );

    assert!(result.is_ok());

    // Verify repository is clean
    let status = git_service.get_status(repo_path).unwrap();
    assert!(status.is_clean);
}

#[serial]
#[test]
fn test_empty_repository_operations() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Repository should be valid but clean
    assert!(git_service.is_repository(repo_path.clone()).unwrap());

    let status = git_service.get_status(repo_path.clone()).unwrap();
    assert!(status.is_clean);
    assert_eq!(status.modified_files.len(), 0);
    assert_eq!(status.untracked_files.len(), 0);
    assert_eq!(status.staged_files.len(), 0);

    // Staged files should be empty
    let staged_files = git_service.get_staged_files(repo_path).unwrap();
    assert_eq!(staged_files.len(), 0);
}

#[serial]
#[test]
fn test_stress_commit_operations() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    // Create many small commits
    for i in 0..50 {
        let filename = format!("commit_{}.txt", i);
        let content = format!("Content for commit {}", i);
        let message = format!("Commit number {}", i);

        test_repo.add_file(&filename, &content).unwrap();

        let result = git_service.commit_file(
            repo_path.clone(),
            filename,
            message,
            "Test User".to_string(),
            "test@example.com".to_string(),
        );

        assert!(result.is_ok(), "Commit {} should succeed", i);
    }

    // Repository should be clean after all commits
    let status = git_service.get_status(repo_path).unwrap();
    assert!(status.is_clean);
}

#[serial]
#[test]
fn test_validation_integration() {
    let test_repo = test_utils::TestRepo::new().unwrap();
    let repo_path = test_repo.path_str().to_string();
    let git_service = create_git_service();

    test_repo.add_file("test.txt", "content").unwrap();

    // Test path traversal protection
    let result = git_service.stage_file(repo_path.clone(), "../outside.txt".to_string());
    assert!(result.is_err());

    // Test empty commit message
    let result = git_service.commit_file(
        repo_path.clone(),
        "test.txt".to_string(),
        "".to_string(),
        "Test User".to_string(),
        "test@example.com".to_string(),
    );
    assert!(result.is_err());

    // Test invalid email
    let result = git_service.commit_file(
        repo_path.clone(),
        "test.txt".to_string(),
        "Valid message".to_string(),
        "Test User".to_string(),
        "invalid-email".to_string(),
    );
    assert!(result.is_err());

    // Test empty user name
    let result = git_service.commit_file(
        repo_path,
        "test.txt".to_string(),
        "Valid message".to_string(),
        "".to_string(),
        "test@example.com".to_string(),
    );
    assert!(result.is_err());
}