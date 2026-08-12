mod common;

/// Remote operations, tested against local bare repositories used as remotes.
///
/// git treats a filesystem path as a perfectly ordinary remote, so fetch and
/// push can be exercised end to end with no network, no server and no
/// credentials. That covers the plumbing — refspecs, ref updates, ahead/behind,
/// rejection handling — which is where the bugs actually are.
///
/// What it does not cover is authentication, because a local path never asks
/// for any. `credential_callback` is therefore exercised by nothing here; that
/// gap is real and is called out in the README rather than papered over.
#[cfg(test)]
mod remote_ops_tests {
    use crate::common::*;
    use liminal_git::RemoteCredentials;

    /// A bare repository to act as the remote, and a working repository with
    /// one commit and `origin` pointing at it.
    fn repo_with_remote() -> (TempDir, String, String) {
        let temp = TempDir::new().unwrap();

        let remote_path = temp.path().join("remote.git");
        git2::Repository::init_bare(&remote_path).unwrap();

        let work_path = temp.path().join("work");
        std::fs::create_dir(&work_path).unwrap();
        let work = work_path.to_string_lossy().to_string();
        init_repository_impl(&work).unwrap();

        let file = work_path.join("a.md");
        fs::write(&file, "one\n").unwrap();
        commit_file_impl(&work, &file.to_string_lossy(), "first", "T", "t@e.com").unwrap();

        add_remote_impl(&work, "origin", &remote_path.to_string_lossy()).unwrap();

        (temp, work, remote_path.to_string_lossy().to_string())
    }

    fn head_branch(repo_path: &str) -> String {
        let repo = git2::Repository::open(repo_path).unwrap();
        let head = repo.head().unwrap();
        head.shorthand().unwrap().to_string()
    }

    // ===== remote management =====

    #[test]
    fn add_and_list_remotes() {
        let (_t, work, remote_path) = repo_with_remote();

        let remotes = list_remotes_impl(&work).unwrap();
        assert_eq!(remotes.len(), 1);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].url.as_deref(), Some(remote_path.as_str()));
        // pushurl is only reported when it differs from the fetch URL.
        assert_eq!(remotes[0].push_url, None);
    }

    #[test]
    fn set_remote_url_changes_it() {
        let (_t, work, _remote) = repo_with_remote();

        set_remote_url_impl(&work, "origin", "https://example.com/x.git").unwrap();

        let remotes = list_remotes_impl(&work).unwrap();
        assert_eq!(remotes[0].url.as_deref(), Some("https://example.com/x.git"));
    }

    /// Setting the URL of a remote that does not exist must fail rather than
    /// quietly writing config for a remote nobody added.
    #[test]
    fn set_remote_url_rejects_unknown_remote() {
        let (_t, work, _remote) = repo_with_remote();

        let result = set_remote_url_impl(&work, "nope", "https://example.com/x.git");

        assert!(result.is_err(), "expected an error, got {result:?}");
        assert!(
            list_remotes_impl(&work)
                .unwrap()
                .iter()
                .all(|r| r.name != "nope")
        );
    }

    #[test]
    fn remove_remote_removes_it() {
        let (_t, work, _remote) = repo_with_remote();

        remove_remote_impl(&work, "origin").unwrap();

        assert!(list_remotes_impl(&work).unwrap().is_empty());
    }

    // ===== push and fetch =====

    #[test]
    fn push_puts_the_branch_on_the_remote() {
        let (_t, work, remote_path) = repo_with_remote();
        let branch = head_branch(&work);

        let result = push_impl(&work, "origin", &branch, RemoteCredentials::default()).unwrap();

        assert_eq!(result.remote, "origin");
        assert_eq!(result.pushed_refs.len(), 1);

        // The remote genuinely has the branch, asserted against the remote
        // repository rather than against our own return value.
        let remote_repo = git2::Repository::open_bare(&remote_path).unwrap();
        let pushed = remote_repo
            .find_reference(&format!("refs/heads/{branch}"))
            .expect("branch should exist on the remote after a push");
        assert!(pushed.target().is_some());
    }

    #[test]
    fn push_rejects_a_branch_that_does_not_exist_locally() {
        let (_t, work, _remote) = repo_with_remote();

        let result = push_impl(
            &work,
            "origin",
            "no-such-branch",
            RemoteCredentials::default(),
        );

        assert!(
            matches!(result, Err(GitError::BranchNotFound { .. })),
            "got {result:?}"
        );
    }

    #[test]
    fn fetch_brings_down_refs() {
        let (_t, work, remote_path) = repo_with_remote();
        let branch = head_branch(&work);
        push_impl(&work, "origin", &branch, RemoteCredentials::default()).unwrap();

        // A second clone of the same remote, which should see that branch.
        let other = TempDir::new().unwrap();
        let other_path = other.path().to_string_lossy().to_string();
        init_repository_impl(&other_path).unwrap();
        add_remote_impl(&other_path, "origin", &remote_path).unwrap();

        let result = fetch_impl(&other_path, "origin", RemoteCredentials::default()).unwrap();

        assert_eq!(result.remote, "origin");
        assert!(
            result.updated_refs.iter().any(|r| r.contains(&branch)),
            "expected {branch} among {:?}",
            result.updated_refs
        );
    }

    // ===== upstream status =====

    /// A branch with no upstream must be distinguishable from a branch that is
    /// level with its upstream. Both report ahead 0 / behind 0, and conflating
    /// them tells the user they are up to date when nothing is known.
    #[test]
    fn upstream_status_flags_a_branch_with_no_upstream() {
        let (_t, work, _remote) = repo_with_remote();
        let branch = head_branch(&work);

        let status = get_upstream_status_impl(&work, &branch).unwrap();

        assert!(status.no_upstream);
        assert_eq!(status.upstream, None);
        assert_eq!((status.ahead, status.behind), (0, 0));
    }

    #[test]
    fn upstream_status_counts_commits_ahead() {
        let (temp, work, _remote) = repo_with_remote();
        let branch = head_branch(&work);
        push_impl(&work, "origin", &branch, RemoteCredentials::default()).unwrap();

        // Establish the tracking relationship the way `push -u` would.
        {
            let repo = git2::Repository::open(&work).unwrap();
            repo.find_reference(&format!("refs/heads/{branch}"))
                .unwrap();
            let mut local = repo.find_branch(&branch, git2::BranchType::Local).unwrap();
            // fetch first so the remote-tracking ref exists locally
            fetch_impl(&work, "origin", RemoteCredentials::default()).unwrap();
            local
                .set_upstream(Some(&format!("origin/{branch}")))
                .expect("set upstream");
        }

        // One local commit that the remote has not seen.
        let file = temp.path().join("work").join("b.md");
        fs::write(&file, "two\n").unwrap();
        commit_file_impl(&work, &file.to_string_lossy(), "second", "T", "t@e.com").unwrap();

        let status = get_upstream_status_impl(&work, &branch).unwrap();

        assert!(!status.no_upstream);
        assert_eq!(
            status.upstream.as_deref(),
            Some(format!("origin/{branch}").as_str())
        );
        assert_eq!(status.ahead, 1, "one unpushed commit");
        assert_eq!(status.behind, 0);
    }

    #[test]
    fn upstream_status_rejects_an_unknown_branch() {
        let (_t, work, _remote) = repo_with_remote();

        let result = get_upstream_status_impl(&work, "no-such-branch");

        assert!(
            matches!(result, Err(GitError::BranchNotFound { .. })),
            "got {result:?}"
        );
    }
}
