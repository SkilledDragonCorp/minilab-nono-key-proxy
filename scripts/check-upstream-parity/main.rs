//! Diff every `run:` script body in `.github/workflows/upstream-ci.yml`
//! against `ci/upstream/ci.yml` for jobs: test, integration, fmt, clippy, audit.
//! Fails on any unlisted difference in those run scripts.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const JOBS: &[&str] = &["test", "integration", "fmt", "clippy", "audit"];

fn main() -> ExitCode {
    let root = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok()
        .and_then(|p| p.parent().map(|p| p.parent().unwrap().to_path_buf()))
        .unwrap_or_else(|| env::current_dir().expect("cwd"));

    let upstream_path = root.join("ci/upstream/ci.yml");
    let lab_path = root.join(".github/workflows/upstream-ci.yml");

    let upstream = fs::read_to_string(&upstream_path).unwrap_or_else(|e| {
        panic!("read {}: {e}", upstream_path.display());
    });
    let lab = fs::read_to_string(&lab_path).unwrap_or_else(|e| {
        panic!("read {}: {e}", lab_path.display());
    });

    let upstream_runs = extract_job_runs(&upstream);
    let lab_runs = extract_job_runs(&lab);

    let mut failed = false;
    for job in JOBS {
        let Some(u) = upstream_runs.get(*job) else {
            eprintln!("ERROR: job `{job}` missing in ci/upstream/ci.yml");
            failed = true;
            continue;
        };
        let Some(l) = lab_runs.get(*job) else {
            eprintln!("ERROR: job `{job}` missing in .github/workflows/upstream-ci.yml");
            failed = true;
            continue;
        };
        if u != l {
            eprintln!("ERROR: run: scripts differ for job `{job}`");
            eprintln!("--- upstream ({}) ---", u.len());
            for (i, s) in u.iter().enumerate() {
                eprintln!("[{i}]\n{s}\n");
            }
            eprintln!("--- lab ({}) ---", l.len());
            for (i, s) in l.iter().enumerate() {
                eprintln!("[{i}]\n{s}\n");
            }
            failed = true;
        } else {
            println!("OK: job `{job}` run: scripts match ({} scripts)", u.len());
        }
    }

    // Allowed deviation A1: every lab run step under those jobs should set working-directory: upstream
    if !lab.contains("working-directory: upstream") {
        eprintln!("ERROR: expected working-directory: upstream (allowed deviation A1)");
        failed = true;
    }
    if !lab.contains("submodules: recursive") {
        eprintln!("ERROR: expected checkout submodules: recursive (allowed deviation A1)");
        failed = true;
    }
    if !lab.contains("persist-credentials: false") {
        eprintln!("ERROR: expected persist-credentials: false (allowed deviation A1)");
        failed = true;
    }
    if !lab.contains("upstream/target") {
        eprintln!("ERROR: expected cache path prefixed with upstream/ (allowed deviation A1)");
        failed = true;
    }

    if failed {
        ExitCode::FAILURE
    } else {
        println!("check-upstream-parity: all listed jobs OK");
        ExitCode::SUCCESS
    }
}

fn extract_job_runs(yaml: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current_job: Option<String> = None;
    let mut in_jobs = false;
    let mut lines = yaml.lines().peekable();

    while let Some(line) = lines.next() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }

        // Top-level job key: two-space indent, ends with ':'
        if let Some(name) = parse_job_key(line) {
            current_job = Some(name);
            continue;
        }

        let Some(job) = current_job.clone() else {
            continue;
        };
        if !JOBS.contains(&job.as_str()) {
            continue;
        }

        let trimmed = line.trim_start();
        if trimmed.starts_with("run: |") {
            let indent = line.len() - line.trim_start().len();
            let mut body = String::new();
            while let Some(next) = lines.peek() {
                let next_indent = next.len() - next.trim_start().len();
                if next.trim().is_empty() {
                    body.push('\n');
                    lines.next();
                    continue;
                }
                if next_indent <= indent {
                    break;
                }
                let content = lines.next().unwrap();
                let strip = indent + 2; // YAML block content indent
                if content.len() >= strip {
                    body.push_str(&content[strip..]);
                } else {
                    body.push_str(content.trim_start());
                }
                body.push('\n');
            }
            out.entry(job).or_default().push(normalize(&body));
        } else if let Some(rest) = trimmed.strip_prefix("run:") {
            let script = rest.trim();
            if !script.is_empty() && !script.starts_with('|') && !script.starts_with('>') {
                out.entry(job).or_default().push(normalize(script));
            }
        }
    }
    out
}

fn parse_job_key(line: &str) -> Option<String> {
    if !line.starts_with("  ") || line.starts_with("   ") {
        return None;
    }
    let rest = line.trim();
    if rest.is_empty() || rest.starts_with('#') || rest.starts_with('-') {
        return None;
    }
    if let Some(name) = rest.strip_suffix(':') {
        if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(name.to_string());
        }
    }
    None
}

fn normalize(s: &str) -> String {
    s.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}
