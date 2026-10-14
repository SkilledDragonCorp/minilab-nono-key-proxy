//! keyproxy-lab CLI: mock upstream + profile writer for the nono key-lock minilab.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use keyproxy_lab::{require_lab_upstream_token, serve_mock, write_profile};
use std::io::{self, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "keyproxy-lab",
    about = "SkilledDragon key-lock lab helpers for nono credential proxy demos"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start a local HTTP mock that accepts only Bearer $LAB_UPSTREAM_TOKEN on GET /v1/ok.
    ServeMock {
        /// Bind port (`0` selects an ephemeral port). Prints the chosen port to stdout.
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Write the lab nono profile JSON for a mock upstream.
    Profile {
        #[arg(long)]
        mock_port: u16,
        #[arg(long)]
        workdir: PathBuf,
        #[arg(long)]
        fake_home: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("keyproxy-lab error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::ServeMock { port } => {
            let token = require_lab_upstream_token()?;
            let listener = TcpListener::bind(("127.0.0.1", port))
                .context("bind ephemeral port for serve-mock")?;
            let chosen = listener.local_addr()?.port();
            drop(listener);

            let mock = serve_mock(chosen, token)?;
            println!("{}", mock.addr.port());
            let _ = io::stdout().flush();

            // Keep the process (and mock thread) alive until killed.
            loop {
                thread::sleep(Duration::from_secs(60));
            }
        }
        Commands::Profile {
            mock_port,
            workdir,
            fake_home,
            out,
        } => {
            if mock_port == 0 {
                bail!("--mock-port must be a concrete port (not 0)");
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            write_profile(mock_port, &workdir, &fake_home, &out)?;
            println!("{}", out.display());
            Ok(())
        }
    }
}
