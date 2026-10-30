# Security policy

axysBrowser is a small, privacy-first browser: the goal is to show the web
with as little data leaving the machine as possible, not to sandbox untrusted
native code. WebKitGTK does the rendering; this project owns the policy layer
(popups, permissions, downloads, storage, external schemes).

## Reporting a vulnerability

Please **do not open a public issue** for a security problem.

Use GitHub's private reporting flow:
[Security advisories](https://github.com/elCDP1/axysBrowser/security/advisories/new).

Include the version (`axysBrowser <version>` in *axys://about*), the steps to
reproduce, and what an attacker gains. You can expect an initial reply within
a week; fixes ship as patch releases.

## What is in scope

- Permission prompts bypassed or attributed to the wrong origin.
- Popups, dialogs or external-scheme launches that fire without a gesture or
  beyond their limits.
- Download filenames escaping the chosen directory, or files written outside
  it.
- Private-window data (history, cookies, permissions) reaching disk or a
  normal window.
- Credentials, tokens or full URLs leaking into logs or the journal.
- Clipboard reads or writes without the configured gate.
- Internal pages (`axys://…`) injecting unescaped page data.

## What is out of scope

- Vulnerabilities in WebKitGTK, GTK or the GPU stack: report those upstream
  (they affect every WebKit browser).
- A compromised local account, malicious extensions (there is no extension
  system) or physical access.
- Sites knowing their own data: a site always sees what you type into it.

## Privacy defaults

- Third-party cookies blocked by default; ITP on.
- Per-origin permission prompts, remembered only when asked.
- Clipboard reads prompt per origin (the "allow without asking" setting never
  covers hostless pages); clipboard writes need a user gesture.
- `mailto:`/`tel:`/`sms:` links open the desktop app; every other external
  scheme asks first, and none can launch without a gesture.
- Private windows use an ephemeral session and an in-memory permission store.
- Logs keep only scheme, host and port — never paths, queries or credentials.
