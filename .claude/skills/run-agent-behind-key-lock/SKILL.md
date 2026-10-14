---
name: run-agent-behind-key-lock
description: Wrap Claude Code or any agent CLI in nono run with credential proxy so the agent never sees the real API token.
---

# Run an agent behind the nono key lock

Pin: `nolabs-ai/nono` `v0.80.0` = `8bb041356d48f771a221e58c98d9b1ff442e76fe`.

## What the lock does

1. Host holds the real secret in `LAB_UPSTREAM_TOKEN` (or `GITHUB_TOKEN` / provider keys).
2. `nono run --profile …` starts a credential proxy and puts a **phantom** token in the sandbox env.
3. The agent calls `$SERVICE_BASE_URL` with the phantom in `Authorization`.
4. The proxy swaps the phantom for the real secret and enforces `endpoint_rules` (non-matching paths get 403).

The sandboxed process never receives the real token in env, files, or logs.

## Generate a lab profile

```bash
export LAB_UPSTREAM_TOKEN="$(openssl rand -hex 32)"
cargo run --locked -p keyproxy-lab -- serve-mock --port 0
# note the printed port, then:
cargo run --locked -p keyproxy-lab -- profile \
  --mock-port <PORT> \
  --workdir "$(pwd)" \
  --fake-home "$HOME" \
  --out /tmp/keyproxy-lab.profile.json
nono profile validate --strict /tmp/keyproxy-lab.profile.json
```

## Wrap Claude Code / any agent CLI

```bash
export LAB_UPSTREAM_TOKEN="…real secret on the host only…"
nono run \
  --profile /tmp/keyproxy-lab.profile.json \
  --workdir "$(pwd)" \
  --allow-cwd \
  -- \
  claude
```

Replace `claude` with your agent binary. Point the agent at `$LAB_MOCK_BASE_URL` (or the provider `*_BASE_URL` nono injects for that credential name).

## Add an endpoint rule

In the profile under `network.custom_credentials.<name>.endpoint_rules`:

```json
{ "method": "GET", "path": "/v1/ok" }
```

Or on the CLI (repeatable):

```bash
nono run --profile … --allow-endpoint "lab_mock:GET:/v1/ok" -- …
```

Patterns: `*` = one path segment, `**` = zero or more.

## Verify the lock (keyless)

```bash
cargo test --locked --test e2e
```

Required: `nono` on `PATH` (`cargo install --locked nono-cli --version 0.80.0`). No cloud API keys.

## Opt-outs used by this lab

- `NONO_NO_UPDATE_CHECK=1`
- `NONO_NO_SAVE_PROMPT=1`
- `DO_NOT_TRACK=1`
