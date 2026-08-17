// remote_ops.rs — remote management and the network operations.
//
// These are the only operations in this crate that talk to anything outside
// the filesystem, which is why git2's "https" and "ssh" features exist at all.
// Everything else is deliberately local.

use crate::errors::GitError;
use crate::{FetchResult, PushResult, RemoteCredentials, RemoteInfo, UpstreamStatus};
use git2::{Cred, CredentialType, FetchOptions, PushOptions, RemoteCallbacks, Repository};
use log::info;
use std::cell::RefCell;

// NAPI imports only when feature is enabled
#[cfg(feature = "napi-binding")]
use crate::GitService;
#[cfg(feature = "napi-binding")]
use napi::bindgen_prelude::*;

// ===== CREDENTIALS =====

/// Build the credential callback git2 invokes during a network operation.
///
/// libgit2 asks for credentials by *type*, and may ask more than once — it
/// retries with a different type when the first attempt is rejected. The
/// callback therefore has to answer for whatever it is handed, and give up
/// cleanly rather than loop.
///
/// The order below is deliberate: explicit credentials first, because a caller
/// that supplied them means them; then the ambient mechanisms a developer
/// already has configured, so a machine with a working `git push` keeps
/// working without the host application knowing anything about keys.
///
///   SSH key      explicit private key path → ssh-agent
///   Username     the username libgit2 parsed from the URL, else "git"
///   HTTPS        explicit username/password → git's credential helper
///
/// `attempts` guards against libgit2 asking for the same thing repeatedly when
/// the credential is wrong: without it a bad password becomes an infinite
/// retry rather than an error.
// `std::result::Result` spelled out throughout this module: under the
// napi-binding feature `napi::bindgen_prelude::*` brings its own `Result` into
// scope, whose error type must be `AsRef<str>`. GitError and git2::Error are
// not, so the unqualified form compiles without the feature and fails with it.
fn credential_callback(
    creds: RemoteCredentials,
) -> impl FnMut(&str, Option<&str>, CredentialType) -> std::result::Result<Cred, git2::Error> {
    let attempts = RefCell::new(0u32);

    move |url: &str, username_from_url: Option<&str>, allowed: CredentialType| {
        {
            let mut n = attempts.borrow_mut();
            *n += 1;
            if *n > 8 {
                return Err(git2::Error::from_str(
                    "authentication failed: exhausted the available credentials",
                ));
            }
        }

        if allowed.contains(CredentialType::SSH_KEY) {
            let user = username_from_url.unwrap_or("git");

            if let Some(ref key) = creds.ssh_private_key_path {
                let private = std::path::Path::new(key);
                // libgit2 accepts the public key separately; deriving it by
                // convention covers the ordinary layout and is harmless when
                // the file is absent.
                let public = private.with_extension("pub");
                let public = if public.exists() { Some(public) } else { None };
                return Cred::ssh_key(
                    user,
                    public.as_deref(),
                    private,
                    creds.ssh_passphrase.as_deref(),
                );
            }

            return Cred::ssh_key_from_agent(user);
        }

        if allowed.contains(CredentialType::USERNAME) {
            return Cred::username(username_from_url.unwrap_or("git"));
        }

        if allowed.contains(CredentialType::USER_PASS_PLAINTEXT) {
            if let (Some(user), Some(pass)) = (&creds.username, &creds.password) {
                return Cred::userpass_plaintext(user, pass);
            }

            // A token with no username: GitHub and GitLab accept any non-empty
            // username alongside a personal access token, so this is a
            // convenience rather than a guess about the server.
            if let Some(pass) = &creds.password {
                return Cred::userpass_plaintext("token", pass);
            }

            let config = git2::Config::open_default()?;
            return Cred::credential_helper(&config, url, username_from_url);
        }

        Err(git2::Error::from_str(
            "authentication failed: no credential type this client can supply was offered",
        ))
    }
}

fn fetch_options(creds: RemoteCredentials) -> (FetchOptions<'static>, ProgressHandle) {
    let received = ProgressHandle::default();
    let sink = received.clone();

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(credential_callback(creds));
    callbacks.transfer_progress(move |stats| {
        sink.set(
            stats.received_objects() as u32,
            stats.received_bytes() as u64,
        );
        true
    });

    let mut opts = FetchOptions::new();
    opts.remote_callbacks(callbacks);
    (opts, received)
}

/// Transfer counters shared with libgit2's progress callback.
///
/// `Rc<Cell<..>>` rather than atomics because the callback and the reader run
/// on the same thread: the whole operation happens inside one `spawn_blocking`
/// task.
#[derive(Clone, Default)]
struct ProgressHandle(std::rc::Rc<std::cell::Cell<(u32, u64)>>);

impl ProgressHandle {
    fn set(&self, objects: u32, bytes: u64) {
        self.0.set((objects, bytes));
    }
    fn get(&self) -> (u32, u64) {
        self.0.get()
    }
}

// ===== REMOTE MANAGEMENT (no network) =====

/// List configured remotes.
pub fn list_remotes_impl(repo_path: &str) -> std::result::Result<Vec<RemoteInfo>, GitError> {
    info!("list_remotes");
    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("list_remotes"))?;

    let names = repo
        .remotes()
        .map_err(|e| GitError::from(e).with_operation("list_remotes"))?;

    let mut remotes = Vec::new();
    for name in names.iter().flatten() {
        let remote = repo
            .find_remote(name)
            .map_err(|e| GitError::from(e).with_operation("find_remote"))?;

        // `pushurl` is only interesting when it differs; reporting it always
        // would imply a distinction the repository has not actually made.
        let url = remote.url().map(str::to_string);
        let push_url = remote
            .pushurl()
            .map(str::to_string)
            .filter(|p| Some(p) != url.as_ref());

        remotes.push(RemoteInfo {
            name: name.to_string(),
            url,
            push_url,
        });
    }

    Ok(remotes)
}

/// Add a remote.
pub fn add_remote_impl(
    repo_path: &str,
    name: &str,
    url: &str,
) -> std::result::Result<RemoteInfo, GitError> {
    info!("add_remote: name={}", name);
    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("add_remote"))?;

    repo.remote(name, url)
        .map_err(|e| GitError::from(e).with_operation("add_remote"))?;

    Ok(RemoteInfo {
        name: name.to_string(),
        url: Some(url.to_string()),
        push_url: None,
    })
}

/// Remove a remote and the remote-tracking branches that belong to it.
pub fn remove_remote_impl(repo_path: &str, name: &str) -> std::result::Result<bool, GitError> {
    info!("remove_remote: name={}", name);
    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("remove_remote"))?;

    repo.remote_delete(name)
        .map_err(|e| GitError::from(e).with_operation("remove_remote"))?;

    Ok(true)
}

/// Change a remote's fetch URL.
pub fn set_remote_url_impl(
    repo_path: &str,
    name: &str,
    url: &str,
) -> std::result::Result<RemoteInfo, GitError> {
    info!("set_remote_url: name={}", name);
    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("set_remote_url"))?;

    // find_remote first so a missing remote is reported as such, rather than
    // silently creating config for a remote that does not exist.
    repo.find_remote(name)
        .map_err(|e| GitError::from(e).with_operation("find_remote"))?;

    repo.remote_set_url(name, url)
        .map_err(|e| GitError::from(e).with_operation("set_remote_url"))?;

    Ok(RemoteInfo {
        name: name.to_string(),
        url: Some(url.to_string()),
        push_url: None,
    })
}

// ===== NETWORK OPERATIONS =====

/// Fetch from a remote, updating remote-tracking branches.
///
/// Does not touch the working tree or any local branch — fetching is safe in
/// the sense that nothing you have can be lost by it.
pub fn fetch_impl(
    repo_path: &str,
    remote_name: &str,
    creds: RemoteCredentials,
) -> std::result::Result<FetchResult, GitError> {
    info!("fetch: remote={}", remote_name);
    let start = std::time::Instant::now();

    let repo =
        Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("fetch"))?;
    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|e| GitError::from(e).with_operation("find_remote"))?;

    let (mut opts, progress) = fetch_options(creds);

    // An empty refspec list tells libgit2 to use the remote's configured
    // fetch refspecs, which is what `git fetch <remote>` does.
    let refspecs: [&str; 0] = [];
    remote
        .fetch(&refspecs, Some(&mut opts), None)
        .map_err(|e| GitError::from(e).with_operation("fetch"))?;

    let mut updated_refs = Vec::new();
    if let Ok(list) = remote.list() {
        for head in list {
            updated_refs.push(head.name().to_string());
        }
    }

    let (received_objects, received_bytes) = progress.get();

    info!(
        "fetch: {} refs in {}ms",
        updated_refs.len(),
        start.elapsed().as_millis()
    );

    Ok(FetchResult {
        remote: remote_name.to_string(),
        updated_refs,
        received_objects,
        received_bytes: received_bytes as f64,
    })
}

/// Push a branch to a remote.
///
/// `branch` is a local branch name such as "main". The refspec is built here
/// rather than accepted from the caller: an arbitrary refspec is a sharp tool
/// (`+` forces, a `:` deletes the remote ref), and this API is not the place
/// to hand that over without the caller asking for it explicitly.
pub fn push_impl(
    repo_path: &str,
    remote_name: &str,
    branch: &str,
    creds: RemoteCredentials,
) -> std::result::Result<PushResult, GitError> {
    info!("push: remote={} branch={}", remote_name, branch);
    let start = std::time::Instant::now();

    let repo = Repository::open(repo_path).map_err(|e| GitError::from(e).with_operation("push"))?;

    // Fail before contacting the network if the branch does not exist locally.
    repo.find_branch(branch, git2::BranchType::Local)
        .map_err(|_| GitError::BranchNotFound {
            name: branch.to_string(),
        })?;

    let mut remote = repo
        .find_remote(remote_name)
        .map_err(|e| GitError::from(e).with_operation("find_remote"))?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(credential_callback(creds));

    // libgit2 reports a rejected ref through this callback rather than by
    // failing the push, so without it a non-fast-forward rejection looks like
    // success.
    let rejection: std::rc::Rc<std::cell::RefCell<Option<String>>> = Default::default();
    let sink = rejection.clone();
    callbacks.push_update_reference(move |refname, status| {
        if let Some(msg) = status {
            *sink.borrow_mut() = Some(format!("{}: {}", refname, msg));
        }
        Ok(())
    });

    let mut opts = PushOptions::new();
    opts.remote_callbacks(callbacks);

    let refspec = format!("refs/heads/{}:refs/heads/{}", branch, branch);
    remote
        .push(&[refspec.as_str()], Some(&mut opts))
        .map_err(|e| GitError::from(e).with_operation("push"))?;

    if let Some(reason) = rejection.borrow().clone() {
        return Err(GitError::GitOperationFailure {
            operation: "push".to_string(),
            class: 0,
            code: 0,
            message: format!("remote rejected the push — {}", reason),
        });
    }

    info!("push: success in {}ms", start.elapsed().as_millis());

    Ok(PushResult {
        remote: remote_name.to_string(),
        pushed_refs: vec![refspec],
    })
}

/// Compare a local branch with its configured upstream.
///
/// Reads only what is already in the repository: it does not fetch, so the
/// answer is as fresh as the last fetch was. Callers wanting current
/// information must fetch first.
pub fn get_upstream_status_impl(
    repo_path: &str,
    branch: &str,
) -> std::result::Result<UpstreamStatus, GitError> {
    info!("get_upstream_status: branch={}", branch);

    let repo = Repository::open(repo_path)
        .map_err(|e| GitError::from(e).with_operation("get_upstream_status"))?;

    let local = repo
        .find_branch(branch, git2::BranchType::Local)
        .map_err(|_| GitError::BranchNotFound {
            name: branch.to_string(),
        })?;

    // No upstream is an ordinary state for a branch that has never been
    // pushed, so it is reported rather than raised.
    let upstream = match local.upstream() {
        Ok(u) => u,
        Err(_) => {
            return Ok(UpstreamStatus {
                branch: branch.to_string(),
                upstream: None,
                ahead: 0,
                behind: 0,
                no_upstream: true,
            });
        }
    };

    let upstream_name = upstream.name().ok().flatten().map(str::to_string);

    let local_oid = local
        .get()
        .target()
        .ok_or_else(|| GitError::BranchNotFound {
            name: branch.to_string(),
        })?;
    let upstream_oid = upstream
        .get()
        .target()
        .ok_or_else(|| GitError::BranchNotFound {
            name: upstream_name.clone().unwrap_or_default(),
        })?;

    let (ahead, behind) = repo
        .graph_ahead_behind(local_oid, upstream_oid)
        .map_err(|e| GitError::from(e).with_operation("graph_ahead_behind"))?;

    Ok(UpstreamStatus {
        branch: branch.to_string(),
        upstream: upstream_name,
        ahead: ahead as u32,
        behind: behind as u32,
        no_upstream: false,
    })
}

// ===== NAPI WRAPPERS =====

#[cfg(feature = "napi-binding")]
pub async fn list_remotes(service: &GitService, repo_path: String) -> Result<Vec<RemoteInfo>> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || list_remotes_impl(&repo_path)).await
}

#[cfg(feature = "napi-binding")]
pub async fn add_remote(
    service: &GitService,
    repo_path: String,
    name: String,
    url: String,
) -> Result<RemoteInfo> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        add_remote_impl(&repo_path, &name, &url)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn remove_remote(service: &GitService, repo_path: String, name: String) -> Result<bool> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        remove_remote_impl(&repo_path, &name)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn set_remote_url(
    service: &GitService,
    repo_path: String,
    name: String,
    url: String,
) -> Result<RemoteInfo> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        set_remote_url_impl(&repo_path, &name, &url)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn fetch(
    service: &GitService,
    repo_path: String,
    remote_name: String,
    credentials: Option<RemoteCredentials>,
) -> Result<FetchResult> {
    let structured = service.feature_flags().structured_errors;
    let creds = credentials.unwrap_or_default();
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        fetch_impl(&repo_path, &remote_name, creds)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn push(
    service: &GitService,
    repo_path: String,
    remote_name: String,
    branch: String,
    credentials: Option<RemoteCredentials>,
) -> Result<PushResult> {
    let structured = service.feature_flags().structured_errors;
    let creds = credentials.unwrap_or_default();
    crate::utils::run_blocking(structured, move || {
        let _guard = crate::utils::lock_repo(&repo_path)?;
        push_impl(&repo_path, &remote_name, &branch, creds)
    })
    .await
}

#[cfg(feature = "napi-binding")]
pub async fn get_upstream_status(
    service: &GitService,
    repo_path: String,
    branch: String,
) -> Result<UpstreamStatus> {
    let structured = service.feature_flags().structured_errors;
    crate::utils::run_blocking(structured, move || {
        get_upstream_status_impl(&repo_path, &branch)
    })
    .await
}
