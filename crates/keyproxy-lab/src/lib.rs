//! Key-lock lab helpers: mock upstream API + nono profile writer.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};

/// Request log entry recorded by the mock upstream.
#[derive(Debug, Clone)]
pub struct MockRequest {
    pub method: String,
    pub url: String,
    pub authorization: Option<String>,
}

/// Handle for a running mock upstream server.
pub struct MockServer {
    pub addr: SocketAddr,
    pub log: Arc<Mutex<Vec<MockRequest>>>,
    _join: thread::JoinHandle<()>,
}

impl MockServer {
    /// Snapshot of requests that reached the mock (not filtered by the proxy).
    pub fn requests(&self) -> Vec<MockRequest> {
        self.log.lock().expect("mock log poisoned").clone()
    }
}

/// Require `LAB_UPSTREAM_TOKEN` in the environment. Never prints the value.
pub fn require_lab_upstream_token() -> Result<String> {
    match std::env::var("LAB_UPSTREAM_TOKEN") {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => bail!("missing required environment variable: LAB_UPSTREAM_TOKEN"),
    }
}

/// Bound a path argument: must be absolute and resolve under `root` when `root` is Some.
pub fn validate_path_under_root(path: &Path, root: Option<&Path>) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().context("resolve cwd")?.join(path)
    };
    let canon = abs
        .canonicalize()
        .with_context(|| {
            format!(
                "path does not exist or cannot be resolved: {}",
                abs.display()
            )
        })
        .unwrap_or(abs.clone());
    if let Some(root) = root {
        let root_canon = root
            .canonicalize()
            .with_context(|| format!("root does not exist: {}", root.display()))?;
        if !canon.starts_with(&root_canon) {
            bail!(
                "path {} escapes allowed root {}",
                canon.display(),
                root_canon.display()
            );
        }
    }
    Ok(canon)
}

/// Validate upstream URL: https, or http only for 127.0.0.1 / localhost.
pub fn validate_upstream_url(url: &str) -> Result<()> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost") {
        return Ok(());
    }
    bail!("upstream URL must be https:// or http://127.0.0.1 (got non-loopback or non-https)");
}

/// Start the mock upstream on `127.0.0.1:port` (`0` = ephemeral). Returns when bound.
pub fn serve_mock(port: u16, expected_token: String) -> Result<MockServer> {
    let server = Server::http(format!("127.0.0.1:{port}"))
        .map_err(|e| anyhow::anyhow!("bind mock server: {e}"))?;
    let addr = server.server_addr().to_ip().expect("ip addr");
    let log: Arc<Mutex<Vec<MockRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let log_thread = Arc::clone(&log);

    let join = thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let method = request.method().as_str().to_string();
            let url = request.url().to_string();
            let authorization = request
                .headers()
                .iter()
                .find(|h| h.field.equiv("Authorization"))
                .map(|h| h.value.as_str().to_string());

            {
                let mut guard = log_thread.lock().expect("mock log poisoned");
                guard.push(MockRequest {
                    method: method.clone(),
                    url: url.clone(),
                    authorization: authorization.clone(),
                });
            }

            // Drain body so the connection stays healthy.
            let mut sink = Vec::new();
            let _ = std::io::Read::read_to_end(&mut request.as_reader(), &mut sink);

            let expected_header = format!("Bearer {expected_token}");
            let response = if method != "GET" {
                Response::from_string("method not allowed").with_status_code(StatusCode(405))
            } else if url != "/v1/ok" && !url.starts_with("/v1/ok?") {
                Response::from_string(r#"{"error":"forbidden"}"#).with_status_code(StatusCode(403))
            } else if authorization.as_deref() != Some(expected_header.as_str()) {
                Response::from_string(r#"{"error":"unauthorized"}"#)
                    .with_status_code(StatusCode(401))
            } else {
                let body = r#"{"ok":true,"source":"keyproxy-lab-mock"}"#;
                let mut resp = Response::from_string(body).with_status_code(StatusCode(200));
                if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]) {
                    resp = resp.with_header(h);
                }
                resp
            };

            let _ = request.respond(response);
        }
    });

    // Brief settle so callers can connect immediately.
    thread::sleep(Duration::from_millis(20));

    Ok(MockServer {
        addr,
        log,
        _join: join,
    })
}

/// Write the lab nono profile JSON for a mock upstream on `mock_port`.
pub fn write_profile(
    mock_port: u16,
    workdir: &Path,
    fake_home: &Path,
    out: &Path,
) -> Result<Value> {
    let _ = workdir;
    let upstream = format!("http://127.0.0.1:{mock_port}");
    validate_upstream_url(&upstream)?;

    let ssh_deny = fake_home.join(".ssh").to_string_lossy().replace('\\', "/");
    let aws_deny = fake_home.join(".aws").to_string_lossy().replace('\\', "/");

    let profile = json!({
        "meta": { "name": "keyproxy-lab" },
        "groups": { "include": ["deny_credentials"] },
        "workdir": { "access": "readwrite" },
        "filesystem": {
            "allow": [],
            "read": [],
            "deny": [ssh_deny, aws_deny]
        },
        "network": {
            "custom_credentials": {
                "lab_mock": {
                    "upstream": upstream,
                    "credential_key": "env://LAB_UPSTREAM_TOKEN",
                    // Required for URI-form credential_key at nono v0.80.0
                    // (crates/nono-proxy/src/server.rs:647-649): without env_var,
                    // no phantom is injected into the child environment.
                    "env_var": "LAB_UPSTREAM_TOKEN",
                    "inject_header": "Authorization",
                    "credential_format": "Bearer {}",
                    "endpoint_rules": [{ "method": "GET", "path": "/v1/ok" }]
                }
            },
            "credentials": ["lab_mock"]
        }
    });

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).context("create profile parent dir")?;
    }
    let mut f = fs::File::create(out).context("create profile file")?;
    let pretty = serde_json::to_string_pretty(&profile)?;
    f.write_all(pretty.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(profile)
}

/// Create fake HOME fixtures (outside workdir): `.ssh/id_ed25519` and `.aws/credentials`.
pub fn seed_fake_home(fake_home: &Path) -> Result<(PathBuf, PathBuf)> {
    let ssh = fake_home.join(".ssh");
    let aws = fake_home.join(".aws");
    fs::create_dir_all(&ssh)?;
    fs::create_dir_all(&aws)?;
    let id_path = ssh.join("id_ed25519");
    let aws_path = aws.join("credentials");
    // Distinct marker content — must never appear on sandbox stdout.
    fs::write(
        &id_path,
        "FAKE_SSH_PRIVATE_KEY_MARKER_KEYPROXY_LAB_DO_NOT_LEAK\n",
    )?;
    fs::write(
        &aws_path,
        "[default]\naws_access_key_id = FAKEKEY\naws_secret_access_key = FAKESECRET\n",
    )?;
    Ok((id_path, aws_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    #[test]
    fn validate_upstream_rejects_non_loopback_http() {
        assert!(validate_upstream_url("http://example.com").is_err());
        assert!(validate_upstream_url("https://api.github.com").is_ok());
        assert!(validate_upstream_url("http://127.0.0.1:9").is_ok());
    }

    #[test]
    fn mock_accepts_only_real_bearer() {
        let token = "unit-test-token-abc".to_string();
        let mock = serve_mock(0, token.clone()).unwrap();
        let port = mock.addr.port();

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect mock");
        write!(
            stream,
            "GET /v1/ok HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("200"), "{buf}");
        assert!(buf.contains("keyproxy-lab-mock"), "{buf}");

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect mock");
        write!(
            stream,
            "GET /v1/ok HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("401"), "{buf}");

        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect mock");
        write!(
            stream,
            "GET /v1/admin HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).unwrap();
        assert!(buf.contains("403"), "{buf}");
    }
}
