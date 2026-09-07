# Privacy (implementation match)

This describes what the current tree does. `PLAN.md` is not the current spec.
The GitHub Pages copy lives in [`docs/i18n.js`](i18n.js) `privacyBody`.

## Files read on the machine

- Windows APIs for CPU, memory, GPU, and system-volume storage. These stay local.
- Settings in `HKCU\Software\SystemExe\RunDog`.
- Claude Code JSONL under `%USERPROFILE%\.claude\projects\` (or `%CLAUDE_CONFIG_DIR%\projects\`).
- Claude credentials at `%USERPROFILE%\.claude\.credentials.json` (or the config-dir equivalent).
- Codex CLI JSONL under `%USERPROFILE%\.codex\sessions\{year}\` (or `%CODEX_HOME%`).
- Codex credentials at `%USERPROFILE%\.codex\auth.json`.
- RunDog usage state under `%LOCALAPPDATA%\SystemExe\RunDog\usage` (written by RunDog, not a vendor home).

RunDog does not spawn `claude`, `codex`, or Node. Oversized JSONL lines are skipped
so a large user blob cannot stall the reader. Limit fetch does not upload JSONL.

## Destinations (vendors, not a RunDog server)

There is **no RunDog-owned server**. Nothing is posted to SystemExe or this
repository except public GitHub Release metadata that GitHub already hosts.

| Destination | When | What is sent |
| --- | --- | --- |
| `api.github.com` | Once at startup, or when the user chooses Check again | Release metadata GET. No account token. |
| `github.com` / GitHub release CDN | After the user chooses Install, or automatically at startup when auto-update on startup is on (default) and a newer release exists | Installer and SHA-256 sidecar GET. |
| `api.anthropic.com` `/api/oauth/usage` | When Claude credentials exist, about every 5 minutes | Bearer access token and Anthropic OAuth beta header. Usage/limit JSON back. |
| `platform.claude.com` or `console.anthropic.com` `/v1/oauth/token` | When the Claude access token is expired or the usage GET fails as unauthorized | OAuth refresh grant. May rewrite `.credentials.json` via `ReplaceFileW`. |
| `chatgpt.com` `/backend-api/wham/usage` | When Codex `auth.json` exists, about every 60 seconds | Bearer access token and ChatGPT account id. Usage/limit JSON back. |

**Not sent** on limit fetch: raw prompts, raw responses, JSONL session bodies,
or tokens to any third party other than the vendor that already issued them.

## Credential refresh and file updates

Claude: if the access token looks expired, or the usage GET comes back
unauthorized, RunDog may refresh and **rewrite** `.credentials.json` in place
(`ReplaceFileW`, no symlink follow). That is the only vendor file it writes.
Codex `auth.json` is read, not rewritten.

Frequency of limit queries is as above. There is no resident poller thread;
short-lived workers run on those periods when the tray loop is already ticking.

## Automatic vendor usage-limit queries (setting deferred)

A tray toggle named `Automatic vendor usage-limit queries` is **not** added.

Reason: today's behavior is already automatic when Claude or Codex credentials
exist. Defaulting a new switch to Off would silently stop limit cards and look
like a product regression. Adding a switch also needs 12-locale menu strings,
registry persistence, and a settings path that this phase must not balloon.
Users who want no vendor queries can remove the CLI credentials or the CLIs.
Existing auto-query behavior is left **On**. It is not turned Off.

## SDKs

There is no advertising, analytics, or crash-reporting SDK.

## Pages / fonts

The GitHub Pages site uses system UI fonts only. It does not load Google Fonts.
Self-hosting Noto would add large webfonts for a static landing page; system
fonts keep CJK/Cyrillic readable without a third-party font CDN.

## SignPath sentence

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it.
GitHub update check and vendor limit queries are those requested operations
(install / running the app with those CLIs present).
