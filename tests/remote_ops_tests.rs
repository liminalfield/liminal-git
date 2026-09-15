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
        commit_file_impl(
            &work,
            &file.to_string_lossy(),
            "first",
            "T",
            "t@e.com",
            None,
        )
        .unwrap();

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
        commit_file_impl(
            &work,
            &file.to_string_lossy(),
            "second",
            "T",
            "t@e.com",
            None,
        )
        .unwrap();

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

    // ===== clone: the destination probe =====

    #[test]
    fn probe_creates_a_missing_destination() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("fresh");

        let state = probe_destination(&dest.to_string_lossy()).unwrap();

        assert!(matches!(state, DestinationState::Created));
        assert!(dest.is_dir(), "the probe creates the directory it reports");
    }

    #[test]
    fn probe_accepts_an_existing_empty_destination() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("empty");
        fs::create_dir(&dest).unwrap();

        let state = probe_destination(&dest.to_string_lossy()).unwrap();

        assert!(matches!(state, DestinationState::ExistingEmpty));
    }

    #[test]
    fn probe_refuses_a_destination_with_files_and_names_one() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("occupied");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join("notes.md"), "mine\n").unwrap();

        let err = probe_destination(&dest.to_string_lossy()).unwrap_err();

        assert_eq!(err.error_code(), "DESTINATION_NOT_EMPTY");
        assert!(
            err.to_string().contains("notes.md"),
            "the refusal names what it found, so a person can explain it: {err}"
        );
        assert_eq!(
            fs::read_to_string(dest.join("notes.md")).unwrap(),
            "mine\n",
            "a refusal touches nothing — the probe runs before anything is created"
        );
    }

    /// A directory holding nothing but a dotfile is not empty, matching
    /// `git clone` and `init_repository_impl`. On macOS a folder containing
    /// only .DS_Store looks empty in Finder, so the refusal has to name the
    /// file or it is unexplainable.
    #[test]
    fn probe_refuses_a_destination_holding_only_a_dotfile() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("dotted");
        fs::create_dir(&dest).unwrap();
        fs::write(dest.join(".DS_Store"), "").unwrap();

        let err = probe_destination(&dest.to_string_lossy()).unwrap_err();

        assert_eq!(err.error_code(), "DESTINATION_NOT_EMPTY");
        assert!(err.to_string().contains(".DS_Store"), "{err}");
    }

    #[test]
    fn probe_refuses_when_the_parent_does_not_exist() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("missing").join("child");

        let err = probe_destination(&dest.to_string_lossy()).unwrap_err();

        assert_eq!(err.error_code(), "INVALID_PATH");
        assert!(err.to_string().contains("parent"), "{err}");
    }

    // ===== clone =====

    /// A bare repository with one commit on its default branch, to clone from.
    fn remote_with_one_commit() -> (TempDir, String) {
        let (temp, work, remote) = repo_with_remote();
        let branch = head_branch(&work);
        push_impl(&work, "origin", &branch, RemoteCredentials::default()).unwrap();
        (temp, remote)
    }

    #[test]
    fn clone_brings_down_a_repository() {
        let (temp, remote) = remote_with_one_commit();
        let dest = temp.path().join("clone");

        let result = clone_impl(
            &remote,
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap();

        assert!(dest.join(".git").is_dir(), "a repository landed");
        assert_eq!(fs::read_to_string(dest.join("a.md")).unwrap(), "one\n");
        assert!(
            result.commit.is_some(),
            "a non-empty remote resolves a commit"
        );
        assert_eq!(result.branch, head_branch(&dest.to_string_lossy()));
        // A local remote is hardlinked rather than transferred, so libgit2's
        // transfer callback never fires and the counters stay 0. That is the
        // honest number: nothing crossed a wire. The clone is proven by the
        // tree and the commit above, not by the counters.
        assert_eq!(result.received_objects, 0);
        assert_eq!(result.received_bytes as u32, 0);
    }

    #[test]
    fn clone_accepts_an_existing_empty_destination() {
        let (temp, remote) = remote_with_one_commit();
        let dest = temp.path().join("prepared");
        fs::create_dir(&dest).unwrap();

        clone_impl(
            &remote,
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap();

        assert!(dest.join(".git").is_dir());
    }

    #[test]
    fn clone_checks_out_the_branch_it_is_given() {
        let (temp, work, remote) = repo_with_remote();
        let default = head_branch(&work);
        push_impl(&work, "origin", &default, RemoteCredentials::default()).unwrap();

        create_branch_impl(
            &work,
            &CreateBranchOptions {
                name: "release".to_string(),
                from_commit: None,
                checkout: true,
            },
        )
        .unwrap();
        let file = std::path::Path::new(&work).join("b.md");
        fs::write(&file, "two\n").unwrap();
        commit_file_impl(
            &work,
            &file.to_string_lossy(),
            "second",
            "T",
            "t@e.com",
            None,
        )
        .unwrap();
        push_impl(&work, "origin", "release", RemoteCredentials::default()).unwrap();

        let dest = temp.path().join("named");
        let result = clone_impl(
            &remote,
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            Some(CloneOptions {
                branch: Some("release".to_string()),
            }),
        )
        .unwrap();

        assert_eq!(result.branch, "release");
        assert!(
            dest.join("b.md").exists(),
            "the named branch was checked out"
        );
    }

    /// An empty remote has no commit, and the branch is unborn rather than
    /// unknown. This test records what libgit2 resolves there — the name
    /// decides whether a host's first commit lands on main or master, so
    /// "absent" would be a worse answer than a name whose provenance is
    /// documented.
    #[test]
    fn clone_of_an_empty_remote_reports_an_unborn_branch() {
        let temp = TempDir::new().unwrap();
        let remote = temp.path().join("empty.git");
        git2::Repository::init_bare(&remote).unwrap();
        let dest = temp.path().join("clone");

        let result = clone_impl(
            &remote.to_string_lossy(),
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap();

        assert!(
            !result.branch.is_empty(),
            "the unborn branch still has a name"
        );
        assert!(result.commit.is_none(), "there is nothing to resolve to");
        assert_eq!(result.received_objects, 0);
    }

    #[test]
    fn clone_removes_a_destination_it_created_when_the_clone_fails() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("doomed");

        let err = clone_impl(
            &temp.path().join("nothing-here.git").to_string_lossy(),
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap_err();

        assert!(
            !dest.exists(),
            "a half-populated directory a later call would treat as a \
             repository is the outcome this is designed to prevent"
        );
        assert!(
            err.to_serializable().details.get("partialRemoved")
                == Some(&serde_json::Value::Bool(true)),
            "{err}"
        );
    }

    #[test]
    fn clone_leaves_a_destination_it_did_not_create() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("mine");
        fs::create_dir(&dest).unwrap();

        let _ = clone_impl(
            &temp.path().join("nothing-here.git").to_string_lossy(),
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap_err();

        assert!(
            dest.is_dir(),
            "we did not create it, so we do not remove it"
        );
        assert_eq!(
            fs::read_dir(&dest).unwrap().count(),
            0,
            "but what landed inside it is ours to clear"
        );
    }

    /// Unix only: a parent with no write bit refuses the destination before
    /// anything exists to clean up — `probe_destination`'s own `create_dir`
    /// fails, so `clone_impl` returns before it ever reaches `clone_failure`.
    /// That is a real, worthwhile property on its own (nothing is left for a
    /// retry to collide with), but it is a different property from cleanup
    /// succeeding or failing, which needs something to already exist — see
    /// `restore_destination`'s own tests in `src/remote_ops.rs` for that.
    /// Skipped as root, where the permission bit does not bite.
    #[cfg(unix)]
    #[test]
    fn clone_refuses_a_destination_whose_parent_is_read_only() {
        use std::os::unix::fs::PermissionsExt;

        if unsafe { libc_geteuid() } == 0 {
            eprintln!("skipped: running as root, where a read-only parent is not read-only");
            return;
        }

        let temp = TempDir::new().unwrap();
        let parent = temp.path().join("locked");
        fs::create_dir(&parent).unwrap();
        let dest = parent.join("clone");

        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o555);
        fs::set_permissions(&parent, perms).unwrap();

        let result = clone_impl(
            &temp.path().join("nothing-here.git").to_string_lossy(),
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        );

        // Restore before asserting, so a failed assertion still lets TempDir
        // clean up rather than leaving the tree behind.
        let mut perms = fs::metadata(&parent).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&parent, perms).unwrap();

        assert!(result.is_err(), "a read-only parent cannot take a clone");
        assert!(
            !dest.exists(),
            "the destination was refused before it was created"
        );
    }

    /// Pins the premise behind the REMOTE_NOT_FOUND arm: that libgit2 reports
    /// a missing remote repository as (Http, NotFound). The classification
    /// tests use synthesised errors and prove the mapping, not the input.
    ///
    /// Ignored because it needs the network. Run it deliberately:
    ///   cargo test --no-default-features -- --ignored clone_reports_a_missing_remote
    ///
    /// Observed against libgit2 1.7.2 (git2 0.18): NOT (Http, NotFound) as
    /// assumed. A real GitHub 404 over HTTPS comes back as class `Http`,
    /// code `GenericError`, message "unexpected http status code: 404" —
    /// libgit2's HTTP transport never sets `GIT_ENOTFOUND` for this case.
    /// `classify_git_failure` in `src/errors.rs` was changed to also match
    /// on that message text so REMOTE_NOT_FOUND still holds and stays
    /// non-retriable; the `NotFound`-code check stays alongside it for the
    /// transports that do set that code for a missing remote.
    #[test]
    #[ignore]
    fn clone_reports_a_missing_remote_repository() {
        let temp = TempDir::new().unwrap();
        let dest = temp.path().join("nope");

        let err = clone_impl(
            "https://github.com/liminalfield/this-repository-does-not-exist.git",
            &dest.to_string_lossy(),
            RemoteCredentials::default(),
            None,
        )
        .unwrap_err();

        let serialized = err.to_serializable();
        assert_eq!(
            serialized.code, "REMOTE_NOT_FOUND",
            "details were {:?} — if libgit2 reports something other than \
             (Http, NotFound), change the arm to match what it actually \
             returns rather than changing this test",
            serialized.details
        );
        assert!(!serialized.retriable, "a typo does not fix itself");
    }

    #[cfg(unix)]
    unsafe extern "C" {
        #[link_name = "geteuid"]
        fn libc_geteuid() -> u32;
    }
}
