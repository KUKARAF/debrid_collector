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
    about = "Generate and run download.sh scripts from real-debrid, powered by AI",
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

        /// Override the AI model (default: qwen/qwen3-32b)
        #[arg(short, long)]
        model: Option<String>,
    },

    /// Run an existing download.sh inside a season directory
    Run {
        /// Path to the directory containing download.sh
        dir: PathBuf,
    },

    /// List available Groq models
    Models,

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
        Cmd::Generate { output_dir, run, dry_run, model } => {
            generate(&output_dir, run, dry_run, model.as_deref()).await?;
        }
        Cmd::Models => {
            let api_key = kv::get_secret("MEDIA_GROQ_API_KEY")
                .context("could not obtain MEDIA_GROQ_API_KEY")?;
            let models = groq::list_models(&api_key).await?;
            if let Some(selected) = select_model(&models)? {
                save_model_to_conventions(selected)?;
                println!("Saved '{selected}' to CONVENTIONS.md");
            }
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

/// Parse optional YAML frontmatter from a markdown string.
/// Returns (frontmatter_model, body_without_frontmatter).
fn parse_frontmatter(src: &str) -> (Option<String>, &str) {
    let Some(rest) = src.strip_prefix("---\n") else {
        return (None, src);
    };
    let Some(end) = rest.find("\n---\n") else {
        return (None, src);
    };
    let front = &rest[..end];
    let body = &rest[end + 5..]; // skip "\n---\n"
    let model = front.lines().find_map(|line| {
        let line = line.trim();
        let val = line.strip_prefix("model:")?;
        Some(val.trim().to_string())
    });
    (model, body)
}

fn save_model_to_conventions(model: &str) -> Result<()> {
    let path = std::path::Path::new("CONVENTIONS.md");
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let (_, body) = parse_frontmatter(&existing);
    let new_content = format!("---\nmodel: {model}\n---\n{body}");
    std::fs::write(path, new_content).context("failed to write CONVENTIONS.md")?;
    Ok(())
}

/// Try to select a model interactively via fzf, falling back to numbered list.
fn select_model(models: &[String]) -> Result<Option<&str>> {
    // Try fzf first
    if std::process::Command::new("fzf").arg("--version").output().is_ok() {
        use std::io::Write as _;
        use std::process::{Command, Stdio};
        let mut child = Command::new("fzf")
            .arg("--prompt=Select model: ")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context("failed to spawn fzf")?;
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(models.join("\n").as_bytes())?;
        }
        let output = child.wait_with_output()?;
        if output.status.success() {
            let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok(models.iter().find(|m| m.as_str() == selected).map(|s| s.as_str()));
        }
        return Ok(None); // user cancelled fzf
    }

    // Fallback: numbered list
    for (i, m) in models.iter().enumerate() {
        println!("{:3}. {}", i + 1, m);
    }
    print!("\nEnter number to save model to CONVENTIONS.md (Enter to skip): ");
    use std::io::Write as _;
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        return Ok(None);
    }
    match input.parse::<usize>() {
        Ok(n) if n >= 1 && n <= models.len() => Ok(Some(&models[n - 1])),
        _ => {
            eprintln!("Invalid selection.");
            Ok(None)
        }
    }
}

async fn generate(output_dir: &PathBuf, run: bool, dry_run: bool, model: Option<&str>) -> Result<()> {
    // ── secrets ──────────────────────────────────────────────────────────────
    eprintln!("[1/4] Loading secrets...");
    let groq_api_key = kv::get_secret("MEDIA_GROQ_API_KEY")
        .context("could not obtain MEDIA_GROQ_API_KEY")?;
    let debrid_token = kv::get_secret("REAL_DEBRID_API_TOKEN")
        .context("could not obtain REAL_DEBRID_API_TOKEN")?;

    // ── conventions ──────────────────────────────────────────────────────────
    let conventions_raw = std::fs::read_to_string("CONVENTIONS.md")
        .context("failed to read CONVENTIONS.md (run from the debrid_collector directory)")?;
    let (frontmatter_model, conventions) = parse_frontmatter(&conventions_raw);

    // Priority: -m flag > CONVENTIONS.md frontmatter > default
    let model = model
        .or(frontmatter_model.as_deref())
        .unwrap_or(groq::DEFAULT_MODEL);

    // ── real-debrid downloads ─────────────────────────────────────────────────
    eprintln!("[2/4] Fetching downloads from real-debrid...");
    let downloads = debrid::list_downloads(&debrid_token).await?;
    eprintln!("      {} downloads found", downloads.len());

    if downloads.is_empty() {
        eprintln!("No downloads found — nothing to do.");
        return Ok(());
    }

    // ── AI call ───────────────────────────────────────────────────────────────
    eprintln!("[3/4] Asking {} to create download.sh files...", model);
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

    let response = groq::chat_with_fallback(&groq_api_key, model, &messages).await?;

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
