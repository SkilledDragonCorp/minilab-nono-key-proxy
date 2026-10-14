# RESULTS

Local green run (2026-10-14T02:56:58Z) before push.

| Field | Value |
| --- | --- |
| `nono --version` | `nono 0.80.0` |
| `uname -r` | `25.6.0` (Darwin) |
| Upstream pin | `8bb041356d48f771a221e58c98d9b1ff442e76fe` (`v0.80.0`) |
| Lab report | `reports/keyproxy.json` |
| CI run URL | https://github.com/SkilledDragonCorp/minilab-nono-key-proxy/actions/runs/38503804585 |
| Upstream CI | https://github.com/SkilledDragonCorp/minilab-nono-key-proxy/actions/runs/38503804584 |

## Exact e2e command lines

```bash
cargo test --workspace --locked --lib
cargo test --workspace --locked --test e2e -- --test-threads=1
bash scripts/report.sh
```

Harness (inside tests) runs variants of:

```bash
nono profile validate --strict <generated>
nono run --profile <generated> --workdir <tmp> --allow-cwd -- \
  sh -c 'curl -s -o body -w "%{http_code}" -H "Authorization: Bearer $LAB_UPSTREAM_TOKEN" "$LAB_MOCK_BASE_URL/v1/ok"; echo; cat body'
nono run --profile <generated> --workdir <tmp> --allow-cwd -- env
nono run --profile <generated> --workdir <tmp> --allow-cwd -- cat <fake-home>/.ssh/id_ed25519
nono run --profile <generated> --workdir <tmp> --allow-cwd -- \
  sh -c 'curl -s -o admin_body -w "%{http_code}" -H "Authorization: Bearer $LAB_UPSTREAM_TOKEN" "$LAB_MOCK_BASE_URL/v1/admin"; echo; cat admin_body'
```

Flags cited from upstream `crates/nono-cli/src/cli.rs` at the pin: `--allow-cwd`, `--profile` / `-p`, `--credential`, `--allow-endpoint`, `--block-net`, `nono profile validate --strict`.

## Pass counts (`reports/keyproxy.json`)

| Step | Status | Duration |
| --- | --- | --- |
| unit_lib | pass | 616ms |
| e2e | pass | 17894ms |
| fmt | pass | 529ms |
| clippy | pass | 459ms |
| skills | pass | 220ms |
| banned | pass | 185ms |
| upstream_parity | pass | 925ms |
| **summary** | **7/0** | **21606ms** |

Profile note: `custom_credentials.lab_mock.env_var = "LAB_UPSTREAM_TOKEN"` is required at this pin for URI-form `credential_key` (`crates/nono-proxy/src/server.rs:647-649`).
