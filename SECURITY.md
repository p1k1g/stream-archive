# Security Policy

## Supported versions

Stream Archive is currently a fast-moving pre-1.0 project. Security fixes are applied to the latest `main` branch and the latest published release when practical. Older development snapshots are not guaranteed to receive backports.

## Reporting a vulnerability

Please avoid publishing credentials, session cookies, API keys, management tokens, private VOD URLs, or exploit details in a public issue.

When GitHub private vulnerability reporting / Security Advisories are available for this repository, use that channel for security-sensitive reports. Include:

- affected version or commit
- operating system
- affected component or endpoint
- reproduction steps
- expected vs. actual behavior
- impact assessment
- relevant logs with secrets removed

If a public issue is the only available contact path, provide only the minimum non-sensitive information needed to establish contact and do not include a working exploit or private credentials.

## Secrets and local data

Stream Archive can store SOOP credentials, Cloudflare Worker API keys, CHZZK authentication cookies, a management token, recording history, and local filesystem paths. These values should never be committed to Git or attached to public issues.

On Windows, protected secrets are stored using CurrentUser DPAPI. Linux/macOS secret-storage support is part of the Phase 20 cross-platform work and must not silently fall back to plaintext storage.

## Remote access

The default server binding is loopback-only (`127.0.0.1:8787`). If remote access is required, keep the Stream Archive server on loopback and terminate HTTPS/authentication at a properly configured reverse proxy as documented in `docs/REVERSE_PROXY.md`.

Do not expose the management token or raw loopback service directly to the public Internet.
