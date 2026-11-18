# The self-hosted Mac runner

The Apple-silicon macOS jobs run on the project's own Mac instead of
GitHub's `macos-15` runners, which were often unavailable ("The job was not
acquired by Runner of type hosted even after multiple attempts").

| Workflow | Job | Where it runs |
| --- | --- | --- |
| `ci.yml` | Workspace (macos-15), Electron bridge validation (macos-15), Performance budgets (macos-15) | This Mac; **pull requests** run on GitHub's `macos-15-intel` |
| `package-macos.yml` | macOS arm64 | This Mac (release and manual runs only) |
| `package-macos.yml` | macOS x86_64 | GitHub's `macos-15-intel` |

The runner is matched by the labels `self-hosted`, `macOS`, `ARM64` and
`kairo-mac`. Until a runner with those labels is online, these jobs wait in
the queue.

## Why pull requests never reach this Mac

This repository is public. A pull request from a fork can change the
workflow or the tests, and a self-hosted runner would run that code on this
machine, next to the signing certificate and release secrets. So pull
requests always use GitHub's runners. Keep it that way: never add
`pull_request` or `pull_request_target` jobs that select `kairo-mac`.

## Set it up

1. **Install the build tools** (once):

   ```bash
   xcode-select --install
   brew install ffmpeg pkgconf nasm
   curl https://sh.rustup.rs -sSf | sh
   ```

2. **Register the runner.** On GitHub: the repository's **Settings → Actions
   → Runners → New self-hosted runner**, choose **macOS** and **ARM64**, and
   run the download and `./config.sh` commands it shows, in a folder such as
   `~/actions-runner`. When `config.sh` asks for extra labels, enter
   `kairo-mac`. Or pass it directly:

   ```bash
   ./config.sh --url https://github.com/CodeLawd/kairo-v2 --token <token from the page> --labels kairo-mac
   ```

3. **Run it as a service**, so it starts when you log in:

   ```bash
   cd ~/actions-runner && ./svc.sh install && ./svc.sh start
   ```

4. **Keep fork pull requests behind approval.** **Settings → Actions →
   General → Approval for running fork pull request workflows from
   contributors**: choose **Require approval for all external contributors**.

5. **Keep the Mac awake while it should take jobs.** **System Settings →
   Battery (or Energy) → Prevent automatic sleeping when the display is
   off.** A sleeping Mac leaves the jobs queued.

## Disk space

The runner keeps its Cargo build output between runs in
`~/.kairo-ci/target/<job>`, so builds stay incremental. Each job starts with
`packaging/ci/trim-runner-cache.sh`, which:

- deletes the job's build output once it passes its limit (40 GiB for
  `workspace`, 20 GiB for `performance`), so that build starts clean;
- deletes every kept build when the disk has less than 20 GiB free, and fails
  the job if that still is not enough.

The arm64 package builds in the job's temp folder, which the runner deletes
after every job. To reclaim the space by hand at any time:

```bash
rm -rf ~/.kairo-ci
```

## What it changes on this Mac

- It uses your Homebrew, Rust toolchain (`rustup toolchain install stable`
  keeps `stable` current) and `~/.cargo` download cache.
- Homebrew packages that are already installed are left as they are
  (`HOMEBREW_NO_AUTO_UPDATE`); a missing one is installed.
- A release run imports the signing certificate into a temporary keychain
  and removes it at the end of the job.
- Jobs use the CPU and GPU while they run, one job at a time.

## Stop or remove it

```bash
cd ~/actions-runner && ./svc.sh stop
./svc.sh uninstall && ./config.sh remove --token <token from Settings → Actions → Runners>
```

With the runner gone, the Mac jobs wait in the queue. To go back to GitHub's
runners, set their `runs-on` back to `macos-15` in `ci.yml` and
`package-macos.yml`.
