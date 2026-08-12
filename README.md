# liminal-git

Git operations for Node.js, built on [libgit2](https://libgit2.org/) via
[napi-rs](https://napi.rs/). No subprocess, no parsing output meant for humans —
the library talks to the repository directly and returns typed data.

It was extracted from [Nocturne Writer](https://github.com/liminalfield/nocturne-writer),
where it provides version control for a writing application, so its 45
operations lean towards the things a content tool needs: file history, a file's
contents at a commit, restoring a deleted file, structured diffs. Branch and tag
management are complete; there are deliberately no network operations (see
[Scope](#scope)).

## Requirements

The addon is compiled from source when you install it, so the installing machine
needs:

- **Rust 1.89 or newer.** Enforced by `rust-version` in `Cargo.toml` and checked
  in CI. The floor is set by `std::fs::File::try_lock`, stabilised in 1.89.
- **A C compiler**, to build the vendored libgit2.
- **Node.js 20 or newer.**

Expect the first install to take roughly 100 seconds while the crate compiles in
release mode. npm does not cache the result between installs.

## Install

```bash
npm install github:liminalfield/liminal-git#v1.0.0
```

Not published to npm. Pinning a tag or a commit is recommended over a branch, so
that a rebuild cannot silently change what you depend on.

## Usage

Every operation except the constructor is asynchronous — 45 of them return a
Promise. Paths are absolute for the repository and repository-relative for files
within it.

```js
const fs = require('node:fs');
const path = require('node:path');
const { GitService } = require('liminal-git');

const git = new GitService();
const repo = '/srv/projects/my-repo';

// initRepository requires the directory to be empty.
await git.initRepository(repo);

// The file has to exist on disk; commitFile stages and commits what is there.
fs.mkdirSync(path.join(repo, 'notes'), { recursive: true });
fs.writeFileSync(path.join(repo, 'notes/chapter-one.md'), '# Chapter One\n');

await git.commitFile(
  repo,
  'notes/chapter-one.md',
  'Add the first chapter',
  'Ada Lovelace',
  'ada@example.com',
);

const status = await git.getStatus(repo);
console.log(status.isClean, status.modifiedFiles);

const history = await git.getCommitHistory(repo, 20, 0);
for (const commit of history.commits) {
  console.log(commit.shortHash, commit.message);
}
```

TypeScript declarations ship with the package and are generated from the Rust
source, so they cannot drift from it — CI fails if they do.

## Concurrency

Mutating operations take a per-repository lock with two layers: an in-process
mutex, and an OS advisory file lock (`flock` / `LockFileEx`) at
`.git/liminal-git.lock`. The second layer is what makes concurrent *processes*
safe, not merely concurrent threads.

The advisory lock is released by the kernel when the file descriptor closes,
including when a process is killed, so a crash cannot leave a repository wedged.
If a lock cannot be acquired within ten seconds the call fails with
`REPOSITORY_LOCKED`, which is retriable.

This excludes other users of *this library*. It does not exclude `git` itself —
a commit run from a terminal knows nothing about `.git/liminal-git.lock`.

## Scope

Remote operations are supported: `listRemotes`, `addRemote`, `removeRemote`,
`setRemoteUrl`, `fetch`, `push` and `getUpstreamStatus`.

**Not** supported, deliberately: `clone`, and `pull` or `merge`. Merging is not
a missing binding — it is a design problem about conflict resolution, and doing
it badly is worse than not doing it.

Credentials are passed **per operation** rather than held by the service. This
library has no business owning secrets; the host application knows where they
came from and how long they may live. Given explicit credentials it uses them;
without them it falls back to ssh-agent and then to git's credential helper, so
a machine where `git push` already works keeps working.

```js
await git.fetch(repo, 'origin', { password: process.env.GITHUB_TOKEN });
await git.push(repo, 'origin', 'main', {
  sshPrivateKeyPath: '/home/me/.ssh/id_ed25519',
});
```

### A runtime dependency on Linux

Enabling HTTPS means OpenSSL, and on Linux it is **dynamically** linked:

```
libssl.so.3     libcrypto.so.3
```

libgit2 and libssh2 are statically linked; OpenSSL is not. So anything
packaging this on Linux must declare that dependency — `openssl` for pacman,
`libssl3` for deb. Formats that cannot declare dependencies, AppImage in
particular, are the weak spot. Windows is unaffected: it uses winhttp and
schannel, and never pulls OpenSSL at all.

git2's `vendored-openssl` feature would link it statically and remove the
runtime dependency, at the cost of build time and of OpenSSL's licence terms
entering [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

### What the tests do and do not cover

Remote operations are tested end to end against **local bare repositories used
as remotes** — git treats a filesystem path as an ordinary remote, so refspecs,
ref updates, ahead/behind and push rejection are all exercised with no network
and no server.

Authentication is not covered, because a local path never asks for any. The
credential callback is exercised by no test, and that gap is real.

## Errors

Errors carry a stable code, a message, a retriable flag, and structured details.
With the `structured_errors` feature flag on, they cross the N-API boundary as
JSON:

```json
{
  "code": "REPOSITORY_LOCKED",
  "message": "Repository is locked by another process: /srv/projects/my-repo (waited 10003ms)",
  "retriable": true,
  "details": { "path": "/srv/projects/my-repo", "waitedMs": 10003 }
}
```

The codes are stable:

- **Repository** — `REPOSITORY_NOT_FOUND`, `REPOSITORY_CORRUPTED`, `INVALID_REPOSITORY`, `REPOSITORY_LOCKED`
- **Files** — `FILE_NOT_FOUND`, `FILE_NOT_IN_REPOSITORY`, `PATH_TRAVERSAL`
- **Operations** — `NOTHING_TO_COMMIT`, `MERGE_CONFLICT`, `UNCOMMITTED_CHANGES`, `UNSTAGED_CHANGES_WOULD_BE_LOST`, `DETACHED_HEAD`, `CONFIG_MISSING`
- **Branches** — `BRANCH_NOT_FOUND`, `BRANCH_ALREADY_EXISTS`, `CANNOT_DELETE_CURRENT_BRANCH`, `BRANCH_NOT_MERGED`
- **Tags** — `TAG_NOT_FOUND`, `TAG_ALREADY_EXISTS`
- **Validation** — `INVALID_PATH`, `INVALID_ARGUMENT`, `INVALID_COMMIT_HASH`, `INVALID_BRANCH_NAME`, `INVALID_TAG_NAME`
- **System** — `IO_ERROR`, `GIT_OPERATION_FAILURE`

Only `IO_ERROR`, `REPOSITORY_CORRUPTED` and `REPOSITORY_LOCKED` are marked
retriable.

## Feature flags

Set via the `LIMINAL_FEATURE_FLAGS` environment variable, read once when a
`GitService` is constructed. Matching is case-insensitive and comma-separated;
partial names do not match.

```bash
export LIMINAL_FEATURE_FLAGS=structured_errors,enhanced_status
```

- `structured_errors` — return the JSON payload above instead of a plain message
- `enhanced_status` — additional metadata on status results
- `enhanced_diff` — populate diff hunks with line content

All default to off.

## Configuration

Checkout behaviour is read from git config rather than passed per call, so it
can be set per repository:

```bash
git config liminal.checkoutStrategy safe    # default: block only on real conflicts
git config liminal.checkoutStrategy force   # overwrite local changes
```

In `safe` mode, `checkoutBranch` blocks only when a file would actually be
overwritten, and reports exactly which — not every dirty file.

## Logging

Set `LIMINAL_LOG` to enable logging to stderr; the value is an
[`env_logger`](https://docs.rs/env_logger) filter string.

```bash
LIMINAL_LOG=info node app.js
```

## Development

```bash
cargo build --no-default-features    # library only
npm run build                        # the Node addon (napi build --release)
```

### Tests

```bash
cargo test --no-default-features
```

228 tests across nine targets. `--no-default-features` is required rather than
preferred: with the `napi-binding` feature on, a test binary fails at the
**linker**, because napi resolves its symbols from the host Node process at run
time and those symbols do not exist in a test executable. Disabling the feature
tests the pure-Rust layer, which is where all the logic lives — `git_service.rs`
is only the N-API adapter over it.

Some tests set `TMPDIR` to work around cross-device link failures on mounted
filesystems and are marked `#[serial_test::serial]`. If you see unexplained
flakiness, `-- --test-threads=1` will tell you whether that is the cause.

### Lint and format

```bash
cargo fmt
cargo clippy --no-default-features --all-targets -- -D warnings
cargo clippy --lib -- -D warnings
```

Both clippy invocations matter: the two feature sets compile different code, and
clean under one can be dirty under the other. CI runs both.

### Coverage

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --no-default-features
```

### CI

The workflow is `workflow_dispatch` only while the repository is private,
because Actions minutes on private repositories are billed. Run it deliberately:

```bash
gh workflow run ci.yml -R liminalfield/liminal-git
gh workflow run ci.yml -R liminalfield/liminal-git -f platforms=all
```

It checks formatting, clippy under both feature sets, the test suite, the
declared MSRV, and — the check that matters most — that the package installs and
loads from a *packed tarball*, not merely that it builds inside a clone. Those
are different things, and the difference once hid a package that shipped every
Rust source file and no binary.

## Architecture

```
src/
├── lib.rs              module exports
├── types.rs            data structures shared with JavaScript
├── feature_flags.rs    feature flag parsing
├── git_service.rs      N-API bindings — the only napi-aware module
├── validation.rs       input validation, returns GitError
├── utils.rs            path handling, repository locking
├── errors.rs           GitError and its JSON serialisation
├── repository_ops.rs   init, status, info, health
├── file_ops.rs         stage, commit, move, restore
├── history_ops.rs      history, diffs, file-at-commit
├── branch_ops.rs       branch management
└── tag_ops.rs          tag management
```

The `*_ops` modules are the library. Each public operation is a `*_impl`
function returning `Result<T, GitError>` with no knowledge of Node, and
`git_service.rs` is a thin layer that validates its arguments, takes the
repository lock, and converts errors at the boundary. That split is why the
tests can run at all, and it is what a second, non-Node consumer would build on.

## License

MIT — see [LICENSE](LICENSE).

The compiled addon is not only this project's code. libgit2 is **statically
linked** into every binary, under GPLv2 *with a linking exception*. That
exception is what lets liminal-git be MIT, and lets anything linking liminal-git
choose its own terms; the GPL still governs libgit2 itself.

Crate metadata will not tell you this — `libgit2-sys` declares
`MIT OR Apache-2.0`, which describes the Rust binding rather than the C library
it vendors and compiles in. See [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md),
which ships with the package.
