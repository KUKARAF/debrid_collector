use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

fn kv_binary() -> PathBuf {
    // Env override
    if let Ok(path) = std::env::var("KV_CLI_PATH") {
        return PathBuf::from(path);
    }

    // Relative to the directory this binary lives in (e.g. debrid_collector/target/release/)
    if let Ok(exe) = std::env::current_exe() {
        // go up: release/ → target/ → debrid_collector/ → dev/ → kv_cli/target/release/kv
        let candidate = exe
            .parent() // release
            .and_then(|p| p.parent()) // target
            .and_then(|p| p.parent()) // debrid_collector
            .and_then(|p| p.parent()) // dev
            .map(|dev| dev.join("kv_cli/target/release/kv"));
        if let Some(p) = candidate {
            if p.exists() {
                return p;
            }
        }
    }

    // Relative to the current working directory (../kv_cli/target/release/kv)
    let candidate = PathBuf::from("../kv_cli/target/release/kv");
    if candidate.exists() {
        return candidate;
    }

    // Fall back: assume `kv` is in PATH
    PathBuf::from("kv")
}

/// Fetch a value from kv.osmosis.page via the kv_cli binary.
pub fn get(key: &str) -> Result<String> {
    let bin = kv_binary();
    let output = Command::new(&bin)
        .args(["get", key])
        .output()
        .with_context(|| format!("failed to run kv binary at '{}'", bin.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("kv get {key} failed: {stderr}");
    }

    let value = String::from_utf8(output.stdout).context("kv output is not valid UTF-8")?;
    Ok(value.trim().to_string())
}

/// Try env var first, then fall back to KV store.
pub fn get_secret(key: &str) -> Result<String> {
    if let Ok(val) = std::env::var(key) {
        if !val.is_empty() {
            return Ok(val);
        }
    }
    eprintln!("  fetching {key} from KV store...");
    get(key)
}
