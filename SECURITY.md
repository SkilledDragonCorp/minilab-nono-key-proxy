# Security Policy

**Built by [SkilledDragon LLC](https://skilleddragon.com)**

This is a **SkilledDragon educational minilab**, not a production product. We do **not** maintain these repos on a regular schedule and we do **not** operate a vulnerability response program for them.

## Supported versions

Only the default branch (`main`) of this minilab is considered current. Older commits are archival.

## Contact

General questions: **hello@skilleddragon.com**  
Do not use a dedicated security mailbox for this repo — none exists.

## If you find an issue

- Prefer reporting **upstream** when the issue is in [nolabs-ai/nono](https://github.com/nolabs-ai/nono) (Apache-2.0) or other dependencies.
- For something specific to this minilab’s mock server, profile writer, or CI, you may email hello@skilleddragon.com. There is **no SLA**, and we may not patch or reply quickly.
- Do not expect private coordinated disclosure or paid bounty handling here.

## Secret-handling policy

- Secrets come from environment variables only (local `.env` loaded by the operator; never committed).
- Required vars are validated at startup; missing names are printed, values are never printed.
- CI is **keyless**: e2e tests mint a random `LAB_UPSTREAM_TOKEN` in-process and never write it to disk or logs.
- Optional live demo: fine-grained `GITHUB_TOKEN` (Metadata: read-only) injected **only** via nono’s credential proxy — never exported into the sandbox.

## Tooling pins

| Check | Pin |
| --- | --- |
| gitleaks | `v8.30.1` (CI downloads the release binary and verifies SHA256) |
| cargo-audit | `0.22.2` — `cargo audit --deny warnings` |
| upstream nono | tag `v0.80.0` = `8bb041356d48f771a221e58c98d9b1ff442e76fe` (Apache-2.0) |

Pre-commit: `.pre-commit-config.yaml` pins the gitleaks hook to the same release tag.

Use at your own risk. Pin versions, review the code, and treat this as sample material.
