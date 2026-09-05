# Code signing policy

Free code signing provided by SignPath.io, certificate by SignPath Foundation.

Status: **pending SignPath Foundation approval**. Current GitHub Release installers
are not yet Authenticode-signed. After approval, only CI-built Windows installers
from this repository will be submitted for signing.

## What will be signed

- `RunDog-Setup-x64.exe` published on [GitHub Releases](https://github.com/tsuyoshi-otake/run-dog/releases)
- The `RunDog.exe` payload inside that installer

Unsigned artifacts (pull request builds, local `cargo build` output) are never
submitted for signing.

## Build and signing process

- Release artifacts are built from this repository by
  [`.github/workflows/release.yml`](.github/workflows/release.yml) on GitHub-hosted
  `windows-latest` runners.
- Only those CI-built artifacts will be submitted to SignPath.
- The private key is held by SignPath (HSM-backed). This project does not store
  the private key.
- Product name metadata on signed binaries is `RunDog`. Product version matches
  the Git tag / `Cargo.toml` version for that build.

## Team roles (single-maintainer project)

- Authors (commit access, can modify the repository without additional reviews):
  - [tsuyoshi-otake](https://github.com/tsuyoshi-otake)
- Reviewers (review required for changes proposed by non-committers, e.g. pull requests):
  - [tsuyoshi-otake](https://github.com/tsuyoshi-otake)
  - Policy: all external pull requests are reviewed by the maintainer before merge.
- Approvers (approve each signing request):
  - [tsuyoshi-otake](https://github.com/tsuyoshi-otake)
  - Policy: each signing request requires explicit approval by the maintainer.

Repository access uses GitHub multi-factor authentication.

## After approval (verification)

SHA-256 sidecars stay mandatory. Signing is not a substitute for the
checksum contract.

When SignPath starts signing CI installers, run
[`scripts/verify-authenticode.ps1`](scripts/verify-authenticode.ps1) on
`dist\RunDog-Setup-x64.exe` and require `Get-AuthenticodeSignature`
`Status = Valid` before `gh release create`. Do not add that gate to
Verify or Release while assets are still unsigned. Toolchain and Inno
pins: [`docs/REPRODUCIBILITY.md`](docs/REPRODUCIBILITY.md).

## Distribution

- Homepage / download: <https://tsuyoshi-otake.github.io/run-dog/>
- Source: <https://github.com/tsuyoshi-otake/run-dog>
- License: MIT (`LICENSE`)

## Privacy policy

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.

Details: [privacy page](https://tsuyoshi-otake.github.io/run-dog/privacy.html).

Network use that the user or installer requests is listed in
[docs/PRIVACY.md](docs/PRIVACY.md). Short form:

- GitHub Releases (`api.github.com` / `github.com`): once at startup, or Check
  again. Download only after Install.
- Claude: `api.anthropic.com` `/api/oauth/usage` about every 5 minutes; token
  refresh may rewrite `.credentials.json`.
- Codex: `chatgpt.com` `/backend-api/wham/usage` about every 60 seconds.
  `auth.json` is read-only.
- There is **no RunDog-owned server**. Limit fetch does not send raw prompts or
  responses. Tokens are never sent to this project or to SignPath.

There is no advertising, analytics, or crash-reporting SDK. A new
`Automatic vendor usage-limit queries` setting is deferred so existing
auto-query UX is not turned Off.

## System changes and uninstall

Installation is per-user (`%LOCALAPPDATA%\Programs\RunDog`). It may create a
Start Menu shortcut, an optional desktop shortcut, and an optional startup
registry value when the user enables launch at logon. Updates replace the same
install path and may close a running `RunDog.exe`.

Uninstall from Windows Settings → Apps → Installed apps → RunDog, or from
Add or Remove Programs. That removes the program files, shortcuts, the
uninstaller entry, the startup Run value, HKCU settings, the usage store, and
the update cache. Claude Code and Codex CLI logs, credentials, and homes are
left untouched. Item-by-item delete / preserve is in
[docs/UNINSTALL.md](docs/UNINSTALL.md). Live uninstall was **NOT RUN** for
this revision.
