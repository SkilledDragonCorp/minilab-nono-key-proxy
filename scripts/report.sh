#!/usr/bin/env bash
# Run keyless lab checks and write reports/keyproxy.json (+ .md).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p reports

START_MS="$(python3 -c 'import time; print(int(time.time()*1000))')"
NONO_VERSION="$(nono --version 2>/dev/null || echo 'nono-not-on-path')"
UNAME_R="$(uname -r)"
UNAME_S="$(uname -s)"

pass=0
fail=0
declare -a results=()

run_step() {
  local name="$1"
  shift
  local t0 t1 dur status
  t0="$(python3 -c 'import time; print(int(time.time()*1000))')"
  if "$@" >"reports/_step_${name}.log" 2>&1; then
    status="pass"
    pass=$((pass + 1))
  else
    status="fail"
    fail=$((fail + 1))
  fi
  t1="$(python3 -c 'import time; print(int(time.time()*1000))')"
  dur=$((t1 - t0))
  results+=("{\"name\":\"${name}\",\"status\":\"${status}\",\"duration_ms\":${dur}}")
  echo "${status} ${name} (${dur}ms)"
}

run_step unit_lib cargo test --workspace --locked --lib --verbose
run_step e2e cargo test --workspace --locked --test e2e --verbose -- --test-threads=1 --nocapture
run_step fmt cargo fmt --all -- --check
run_step clippy cargo clippy --workspace --all-targets --locked -- -D warnings
run_step skills bash scripts/check-skills
run_step banned bash scripts/check-banned-phrases
run_step upstream_parity cargo run --manifest-path scripts/check-upstream-parity/Cargo.toml

END_MS="$(python3 -c 'import time; print(int(time.time()*1000))')"
TOTAL=$((END_MS - START_MS))
GENERATED_AT="$(python3 -c 'import datetime; print(datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"))')"

STEPS_JSON="$(IFS=,; echo "${results[*]}")"

cat > reports/keyproxy.json <<EOF
{
  "lab": "minilab-nono-key-proxy",
  "generated_at": "${GENERATED_AT}",
  "keyless": true,
  "upstream_pin": "8bb041356d48f771a221e58c98d9b1ff442e76fe",
  "upstream_tag": "v0.80.0",
  "nono_version": $(python3 -c "import json,os; print(json.dumps(os.environ.get('NONO_VERSION','${NONO_VERSION}')))" ),
  "uname": {"sysname": "${UNAME_S}", "release": "${UNAME_R}"},
  "summary": {"pass": ${pass}, "fail": ${fail}, "duration_ms": ${TOTAL}},
  "steps": [${STEPS_JSON}]
}
EOF

# Fix nono_version quoting properly
python3 - <<PY
import json
from pathlib import Path
p = Path("reports/keyproxy.json")
data = json.loads(p.read_text())
data["nono_version"] = """${NONO_VERSION}""".strip()
p.write_text(json.dumps(data, indent=2) + "\n")
md = Path("reports/keyproxy.md")
lines = [
  "# keyproxy-lab report",
  "",
  f"- generated_at: {data['generated_at']}",
  f"- nono_version: {data['nono_version']}",
  f"- uname: {data['uname']['sysname']} {data['uname']['release']}",
  f"- upstream: {data['upstream_tag']} ({data['upstream_pin']})",
  f"- pass/fail: {data['summary']['pass']}/{data['summary']['fail']}",
  f"- duration_ms: {data['summary']['duration_ms']}",
  "",
  "## Steps",
  "",
]
for s in data["steps"]:
    lines.append(f"- {s['status']}: {s['name']} ({s['duration_ms']}ms)")
md.write_text("\n".join(lines) + "\n")
print(json.dumps(data["summary"]))
PY

if (( fail > 0 )); then
  exit 1
fi
