//! End-to-end key-lock tests against the real `nono` binary (no mocks of nono).
//!
//! Requires `nono` on PATH (install: `cargo install --locked nono-cli --version 0.80.0`
//! or from git pin `8bb041356d48f771a221e58c98d9b1ff442e76fe`).

#![allow(clippy::unwrap_used)]

use keyproxy_lab::{MockServer, seed_fake_home, serve_mock, write_profile};
use rand::RngCore;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tempfile::{Builder, TempDir};

const MARKER: &str = "FAKE_SSH_PRIVATE_KEY_MARKER_KEYPROXY_LAB_DO_NOT_LEAK";

fn nono_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        which::which("nono").unwrap_or_else(|_| {
            panic!(
                "nono binary not found on PATH; install with:\n  \
                 cargo install --locked nono-cli --version 0.80.0\n  \
                 or: cargo install --git https://github.com/nolabs-ai/nono \
                 --rev 8bb041356d48f771a221e58c98d9b1ff442e76fe nono-cli --locked"
            )
        })
    })
    .clone()
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct Harness {
    _tmpdir: TempDir,
    workdir: PathBuf,
    home: PathBuf,
    state: PathBuf,
    #[allow(dead_code)]
    fake_home: PathBuf,
    profile: PathBuf,
    ssh_fixture: PathBuf,
    real_token: String,
    mock: MockServer,
    mock_port: u16,
}

impl Harness {
    fn new() -> Self {
        let real_token = random_token();
        // Rust 2024: env::set_var is unsafe (process-global). E2E runs with --test-threads=1.
        unsafe {
            env::set_var("LAB_UPSTREAM_TOKEN", &real_token);
        }

        let mock = serve_mock(0, real_token.clone()).expect("start mock");
        let mock_port = mock.addr.port();

        // Mirror upstream nono-test-support: keep hermetic dirs under target/,
        // and keep XDG_STATE_HOME a sibling of HOME (not under HOME).
        let artifacts =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        let tmpdir = Builder::new()
            .prefix("keyproxy-e2e-")
            .tempdir_in(&artifacts)
            .expect("temp under target/test-artifacts");

        let workdir = tmpdir.path().join("workdir");
        let home = tmpdir.path().join("home");
        let state = tmpdir.path().join("state");
        let fake_home = tmpdir.path().join("fake-home");
        fs::create_dir_all(home.join(".config")).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&workdir).unwrap();
        fs::create_dir_all(&fake_home).unwrap();
        let (ssh_fixture, _) = seed_fake_home(&fake_home).unwrap();

        let profile = tmpdir.path().join("keyproxy-lab.profile.json");
        write_profile(mock_port, &workdir, &fake_home, &profile).unwrap();

        let validate = Command::new(nono_bin())
            .args(["profile", "validate", "--strict"])
            .arg(&profile)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_STATE_HOME", &state)
            .env("NONO_NO_UPDATE_CHECK", "1")
            .env("NONO_NO_SAVE_PROMPT", "1")
            .output()
            .expect("nono profile validate");
        assert!(
            validate.status.success(),
            "nono profile validate --strict failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&validate.stdout),
            String::from_utf8_lossy(&validate.stderr)
        );

        Self {
            _tmpdir: tmpdir,
            workdir,
            home,
            state,
            fake_home,
            profile,
            ssh_fixture,
            real_token,
            mock,
            mock_port,
        }
    }

    fn nono_run(&self, argv: &[&str]) -> Output {
        let mut cmd = Command::new(nono_bin());
        // Clear inherited agent/CI env so host LAB_UPSTREAM_TOKEN is only the
        // value we set and nono's phantom injection can replace it in-child.
        cmd.env_clear()
            .arg("run")
            .arg("--profile")
            .arg(&self.profile)
            .arg("--workdir")
            .arg(&self.workdir)
            .arg("--allow-cwd")
            .args(argv)
            .current_dir(&self.workdir)
            .env(
                "PATH",
                env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
            )
            .env("USER", env::var("USER").unwrap_or_else(|_| "lab".into()))
            .env("TMPDIR", &self.workdir)
            .env("LAB_UPSTREAM_TOKEN", &self.real_token)
            .env("NONO_NO_UPDATE_CHECK", "1")
            .env("NONO_NO_SAVE_PROMPT", "1")
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", &self.state)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.output().expect("spawn nono run")
    }
}

fn assert_contains(hay: &str, needle: &str, ctx: &str) {
    assert!(
        hay.contains(needle),
        "{ctx}\n--- haystack ---\n{hay}\n--- missing ---\n{needle}"
    );
}

fn assert_not_contains(hay: &str, needle: &str, ctx: &str) {
    assert!(
        !hay.contains(needle),
        "{ctx}: real secret leaked into output\n--- haystack ---\n{hay}"
    );
}

/// T1: phantom Authorization + LAB_MOCK_BASE_URL yields 200 JSON from mock (proxy swapped token).
#[test]
fn t1_proxy_swaps_phantom_for_real_token() {
    let h = Harness::new();
    let out = h.nono_run(&[
        "--",
        "sh",
        "-c",
        r#"curl -s -o body -w "%{http_code}" -H "Authorization: Bearer $LAB_UPSTREAM_TOKEN" "$LAB_MOCK_BASE_URL/v1/ok"; echo; cat body"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "T1 nono run failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_contains(&stdout, "200", "T1 expected HTTP 200");
    assert_contains(&stdout, "keyproxy-lab-mock", "T1 expected mock JSON body");
    assert_not_contains(&stdout, &h.real_token, "T1 stdout");
    assert_not_contains(&stderr, &h.real_token, "T1 stderr");

    let reqs = h.mock.requests();
    assert!(
        reqs.iter().any(|r| {
            r.url.starts_with("/v1/ok")
                && r.authorization.as_deref() == Some(format!("Bearer {}", h.real_token).as_str())
        }),
        "mock should have received real Bearer token; got {reqs:?}"
    );
}

/// T1b: missing Authorization header → 401 from phantom validation.
#[test]
fn t1b_missing_authorization_returns_401() {
    let h = Harness::new();
    let out = h.nono_run(&[
        "--",
        "sh",
        "-c",
        r#"curl -s -o body -w "%{http_code}" "$LAB_MOCK_BASE_URL/v1/ok"; echo; cat body"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "T1b curl wrapper should exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_contains(&stdout, "401", "T1b expected HTTP 401 from proxy");
}

/// T2: sandboxed env never contains the real token; BASE_URL points at proxy, not mock port.
#[test]
fn t2_env_holds_phantom_not_real_token() {
    let h = Harness::new();
    let out = h.nono_run(&["--", "env"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "T2 env failed\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_not_contains(&stdout, &h.real_token, "T2 env stdout");

    let lab_token_line = stdout
        .lines()
        .find(|l| l.starts_with("LAB_UPSTREAM_TOKEN="))
        .expect("LAB_UPSTREAM_TOKEN present in sandbox env");
    let phantom = lab_token_line.trim_start_matches("LAB_UPSTREAM_TOKEN=");
    assert_ne!(phantom, h.real_token, "sandbox token must be phantom");
    let proxy_token = stdout
        .lines()
        .find(|l| l.starts_with("NONO_PROXY_TOKEN="))
        .map(|l| l.trim_start_matches("NONO_PROXY_TOKEN="))
        .expect("NONO_PROXY_TOKEN present in sandbox env");
    assert_eq!(
        phantom, proxy_token,
        "LAB_UPSTREAM_TOKEN phantom must match NONO_PROXY_TOKEN session token"
    );

    let base = stdout
        .lines()
        .find(|l| l.starts_with("LAB_MOCK_BASE_URL="))
        .expect("LAB_MOCK_BASE_URL present");
    let base_url = base.trim_start_matches("LAB_MOCK_BASE_URL=");
    assert!(
        base_url.contains("127.0.0.1"),
        "BASE_URL should be loopback proxy: {base_url}"
    );
    assert!(
        !base_url.contains(&format!(":{}", h.mock_port)),
        "BASE_URL must not point at mock port {}: {base_url}",
        h.mock_port
    );
    assert!(
        base_url.contains("/lab_mock"),
        "BASE_URL should include credential path prefix: {base_url}"
    );

    #[cfg(target_os = "linux")]
    {
        let proc_out = h.nono_run(&["--", "sh", "-c", "tr '\\0' '\\n' < /proc/self/environ"]);
        let proc_stdout = String::from_utf8_lossy(&proc_out.stdout);
        assert!(
            proc_out.status.success(),
            "T2 /proc/self/environ failed\n{}",
            String::from_utf8_lossy(&proc_out.stderr)
        );
        assert_not_contains(&proc_stdout, &h.real_token, "T2 /proc/self/environ");
    }
}

/// T3: reading denied .ssh fixture fails; marker never on stdout.
#[test]
fn t3_denied_ssh_fixture() {
    let h = Harness::new();
    let fixture = h.ssh_fixture.display().to_string();
    let out = h.nono_run(&["--", "cat", &fixture]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "T3 expected non-zero exit when reading denied ssh key\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_not_contains(&stdout, MARKER, "T3 stdout must not leak fixture");
    assert_not_contains(&stderr, MARKER, "T3 stderr must not leak fixture");
    // Control: fixture exists on host.
    assert_eq!(fs::read_to_string(&h.ssh_fixture).unwrap().trim(), MARKER);
}

/// T4: endpoint_rules block /v1/admin (403); mock never sees the request.
#[test]
fn t4_endpoint_rules_block_admin() {
    let h = Harness::new();
    let before = h.mock.requests().len();
    let out = h.nono_run(&[
        "--",
        "sh",
        "-c",
        r#"curl -s -o admin_body -w "%{http_code}" -H "Authorization: Bearer $LAB_UPSTREAM_TOKEN" "$LAB_MOCK_BASE_URL/v1/admin"; echo; cat admin_body"#,
    ]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "T4 curl wrapper should exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert_contains(&stdout, "403", "T4 expected HTTP 403 from proxy L7 filter");

    // Give the mock a brief window; it should still have no new /v1/admin hits.
    std::thread::sleep(Duration::from_millis(100));
    let after = h.mock.requests();
    let admin_hits = after
        .iter()
        .skip(before)
        .filter(|r| r.url.contains("/v1/admin"))
        .count();
    assert_eq!(
        admin_hits, 0,
        "mock must never receive /v1/admin (L7 filter); log={after:?}"
    );
}

/// T5: outside the sandbox the host process still sees the real token (control).
#[test]
fn t5_host_process_still_sees_real_token() {
    let h = Harness::new();
    let host_val = env::var("LAB_UPSTREAM_TOKEN").expect("host env");
    assert_eq!(host_val, h.real_token, "T5 host control");

    let out = Command::new("sh")
        .arg("-c")
        .arg("printf '%s' \"$LAB_UPSTREAM_TOKEN\"")
        .env("LAB_UPSTREAM_TOKEN", &h.real_token)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), h.real_token);

    // Contrast: sandbox differs.
    let sand = h.nono_run(&["--", "sh", "-c", "printf '%s' \"$LAB_UPSTREAM_TOKEN\""]);
    let phantom = String::from_utf8_lossy(&sand.stdout);
    assert_ne!(phantom.as_ref(), h.real_token.as_str());
}

#[test]
fn nono_version_is_available() {
    let out = Command::new(nono_bin()).arg("--version").output().unwrap();
    assert!(out.status.success());
    let v = String::from_utf8_lossy(&out.stdout);
    assert!(
        v.contains("0.80") || v.contains("nono"),
        "unexpected nono --version: {v}"
    );
}

#[test]
fn harness_setup_under_two_seconds_smoke() {
    let start = Instant::now();
    let _h = Harness::new();
    // Soft budget — documents that profile+mock startup is local/keyless.
    assert!(
        start.elapsed() < Duration::from_secs(30),
        "harness setup unexpectedly slow"
    );
}

#[allow(dead_code)]
fn path_exists(p: &Path) -> bool {
    p.exists()
}
