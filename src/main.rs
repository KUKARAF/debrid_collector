mod debrid;
mod downloader;
mod groq;
mod kv;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "debrid-collector",
    about = "Generate and run download.sh scripts from real-debrid, powered by Kimi K2",
    version = env!("APP_VERSION")
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Fetch real-debrid downloads and use AI to generate download.sh files
    Generate {
        /// Output directory for the folder structure (default: current dir)
        #[arg(short, long, default_value = ".")]
        output_dir: PathBuf,

        /// Run generated scripts immediately after writing
        #[arg(long)]
        run: bool,

        /// Print what would be written without creating any files
        #[arg(long)]
        dry_run: bool,
    },

    /// Run an existing download.sh inside a season directory
    Run {
        /// Path to the directory containing download.sh
        dir: PathBuf,
    },

    /// Manage torrents added on Real-Debrid
    Torrent {
        #[command(subcommand)]
        action: TorrentCmd,
    },
}

#[derive(Subcommand)]
enum TorrentCmd {
    /// List all completed (100%) torrents
    Show,
    /// Unrestrict torrent links and push to downloads
    Download {
        /// Glob pattern to match filenames, e.g. "02 - Else*"
        pattern: Option<String>,
    },
    /// Delete torrents from Real-Debrid
    Delete {
        /// Glob pattern to match filenames, e.g. "02 - Else*"
        pattern: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Cmd::Generate { output_dir, run, dry_run } => {
            generate(&output_dir, run, dry_run).await?;
        }
        Cmd::Run { dir } => {
            let script = dir.join("download.sh");
            downloader::run_script(&script)?;
        }
        Cmd::Torrent { action } => {
            torrent_cmd(action).await?;
        }
    }

    Ok(())
}

fn matches_glob(name: &str, pattern: &str) -> bool {
    let name = name.to_lowercase();
    let pattern = pattern.to_lowercase();
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name.contains(parts[0]);
    }
    let mut rest = name.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if i == 0 {
            if !rest.starts_with(part) { return false; }
            rest = &rest[part.len()..];
        } else {
            match rest.find(part) {
                Some(pos) => rest = &rest[pos + part.len()..],
                None => return false,
            }
        }
    }
    if !pattern.ends_with('*') && !rest.is_empty() { return false; }
    true
}

async fn torrent_cmd(action: TorrentCmd) -> Result<()> {
    let token = kv::get_secret("REAL_DEBRID_API_TOKEN")
        .context("could not obtain REAL_DEBRID_API_TOKEN")?;
    let torrents = debrid::list_torrents(&token).await?;

    let total = torrents.len();
    let complete: Vec<_> = torrents.into_iter().filter(|t| t.progress >= 100.0).collect();
    let skipped = total - complete.len();

    match action {
        TorrentCmd::Show => {
            println!("{:<20} {:<50} {:<10} {}", "ID", "FILENAME", "SIZE", "ADDED");
            println!("{}", "-".repeat(95));
            for t in &complete {
                let size = format_bytes(t.bytes);
                let added = t.added.get(..10).unwrap_or(&t.added);
                println!("{:<20} {:<50} {:<10} {}", t.id, truncate(&t.filename, 50), size, added);
            }
            if skipped > 0 {
                eprintln!("({skipped} incomplete torrent(s) not shown)");
            }
        }
        TorrentCmd::Download { pattern } => {
            let selected: Vec<_> = match pattern {
                Some(ref p) => complete.into_iter().filter(|t| matches_glob(&t.filename, p)).collect(),
                None => complete,
            };
            if selected.is_empty() {
                bail!("no matching completed torrents found");
            }
            for torrent in selected {
                let info = debrid::torrent_info(&token, &torrent.id).await?;
                for link in info.links {
                    let result = debrid::unrestrict_link(&token, &link).await?;
                    println!("{}", result.download);
                }
            }
        }
        TorrentCmd::Delete { pattern } => {
            let selected: Vec<_> = match pattern {
                Some(ref p) => complete.into_iter().filter(|t| matches_glob(&t.filename, p)).collect(),
                None => complete,
            };
            if selected.is_empty() {
                bail!("no matching completed torrents found");
            }
            for torrent in &selected {
                debrid::delete_torrent(&token, &torrent.id).await?;
                println!("deleted {}", torrent.filename);
            }
        }
    }

    Ok(())
}

fn format_bytes(bytes: i64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{:.0} KB", bytes as f64 / 1_024.0)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

async fn generate(output_dir: &PathBuf, run: bool, dry_run: bool) -> Result<()> {
    // ── secrets ──────────────────────────────────────────────────────────────
    eprintln!("[1/4] Loading secrets...");
    let groq_api_key = kv::get_secret("MEDIA_GROQ_API_KEY")
        .context("could not obtain MEDIA_GROQ_API_KEY")?;
    let debrid_token = kv::get_secret("REAL_DEBRID_API_TOKEN")
        .context("could not obtain REAL_DEBRID_API_TOKEN")?;

    // ── conventions ──────────────────────────────────────────────────────────
    let conventions = std::fs::read_to_string("CONVENTIONS.md")
        .context("failed to read CONVENTIONS.md (run from the debrid_collector directory)")?;

    // ── real-debrid downloads ─────────────────────────────────────────────────
    eprintln!("[2/4] Fetching downloads from real-debrid...");
    let downloads = debrid::list_downloads(&debrid_token).await?;
    eprintln!("      {} downloads found", downloads.len());

    if downloads.is_empty() {
        eprintln!("No downloads found — nothing to do.");
        return Ok(());
    }

    // ── AI call ───────────────────────────────────────────────────────────────
    eprintln!("[3/4] Asking {} to create download.sh files...", groq::MODEL);
    let downloads_json = serde_json::to_string_pretty(&downloads)?;

    let system_msg = format!(
        "You are a media file organizer. \
         Given a list of real-debrid downloads and naming conventions, \
         you group the files into the correct show/season folder structure \
         and emit the wget commands for each download.sh script.\n\n\
         CONVENTIONS:\n{conventions}\n\n\
         Respond with ONLY a JSON object matching this exact schema (no markdown):\n\
         {{\"scripts\":[{{\"path\":\"Show Title [imdbid-ttXXXXXXX]/S01\",\
         \"content\":\"wget -O \\\"S01E01 - Title.mkv\\\" \\\"https://...\\\"\\n\"}}]}}\n\n\
         Rules:\n\
         - path is relative to the output directory\n\
         - Use the 'download' field from each entry as the wget URL\n\
         - Output filename must follow the conventions (include season+episode, original title language)\n\
         - One wget -O line per file; use double-quotes around filename and URL\n\
         - Group episodes from the same show+season into the same script\n\
         - Omit entries that are not TV show episodes (movies, samples, etc.)\n\
         - If you cannot determine the show/season from the filename, use best judgement"
    );

    let user_msg = format!(
        "Here are my current real-debrid downloads:\n{downloads_json}\n\n\
         Please create the download.sh files."
    );

    let messages = vec![
        groq::Message { role: "system".to_string(), content: system_msg },
        groq::Message { role: "user".to_string(), content: user_msg },
    ];

    let response = groq::chat(&groq_api_key, &messages).await?;

    // ── parse response ────────────────────────────────────────────────────────
    #[derive(serde::Deserialize)]
    struct AiResponse {
        scripts: Vec<downloader::DownloadScript>,
    }

    let ai: AiResponse = serde_json::from_str(&response)
        .with_context(|| format!("AI returned invalid JSON:\n{response}"))?;

    eprintln!("      AI proposed {} script(s)", ai.scripts.len());

    // ── write scripts ─────────────────────────────────────────────────────────
    eprintln!("[4/4] Writing scripts to {}...", output_dir.display());
    let created = downloader::write_scripts(ai.scripts, output_dir, dry_run)?;

    if dry_run {
        eprintln!("Dry-run complete — no files written.");
        return Ok(());
    }

    eprintln!("Created {} script(s).", created.len());

    // ── optional run ──────────────────────────────────────────────────────────
    if run {
        for script_path in &created {
            downloader::run_script(script_path)?;
        }
    }

    Ok(())
}
