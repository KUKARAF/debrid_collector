mod ai;
mod debrid;
mod downloader;
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
            let api_key = kv::get_secret("OPENROUTER_API_KEY")
                .context("could not obtain OPENROUTER_API_KEY")?;
            let models = ai::list_models(&api_key).await?;
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

fn load_conventions(output_dir: &std::path::Path) -> Result<String> {
    let in_output_dir = output_dir.join("CONVENTIONS.md");
    if in_output_dir.exists() {
        return std::fs::read_to_string(&in_output_dir)
            .with_context(|| format!("failed to read {}", in_output_dir.display()));
    }
    let in_cwd = std::path::Path::new("CONVENTIONS.md");
    if in_cwd.exists() {
        return std::fs::read_to_string(in_cwd)
            .context("failed to read ./CONVENTIONS.md");
    }
    bail!(
        "CONVENTIONS.md not found in {} or the current directory",
        output_dir.display()
    )
}

fn scan_output_dir(output_dir: &std::path::Path) -> Result<String> {
    if !output_dir.exists() {
        return Ok(String::new());
    }
    let mut lines: Vec<String> = Vec::new();
    let mut shows: Vec<_> = std::fs::read_dir(output_dir)
        .with_context(|| format!("failed to read {}", output_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    shows.sort_by_key(|e| e.file_name());
    for show in shows {
        lines.push(format!("{}/", show.file_name().to_string_lossy()));
        let mut seasons: Vec<_> = std::fs::read_dir(show.path())
            .unwrap_or_else(|_| panic!())
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        seasons.sort_by_key(|e| e.file_name());
        for season in seasons {
            lines.push(format!("  {}/", season.file_name().to_string_lossy()));
        }
    }
    Ok(lines.join("\n"))
}

async fn generate(output_dir: &PathBuf, run: bool, dry_run: bool, model: Option<&str>) -> Result<()> {
    // ── secrets ──────────────────────────────────────────────────────────────
    eprintln!("[1/5] Loading secrets...");
    let openrouter_api_key = kv::get_secret("OPENROUTER_API_KEY")
        .context("could not obtain OPENROUTER_API_KEY")?;
    let debrid_token = kv::get_secret("REAL_DEBRID_API_TOKEN")
        .context("could not obtain REAL_DEBRID_API_TOKEN")?;

    // ── conventions ──────────────────────────────────────────────────────────
    let conventions_raw = load_conventions(output_dir)?;
    let (frontmatter_model, conventions) = parse_frontmatter(&conventions_raw);

    // Priority: -m flag > CONVENTIONS.md frontmatter > default
    let model = model
        .or(frontmatter_model.as_deref())
        .unwrap_or(ai::DEFAULT_MODEL);

    // ── scan existing structure ───────────────────────────────────────────────
    eprintln!("[2/5] Scanning existing structure in {}...", output_dir.display());
    let existing_structure = scan_output_dir(output_dir)?;
    if existing_structure.is_empty() {
        eprintln!("      (empty — first run)");
    } else {
        let count = existing_structure.lines().filter(|l| !l.starts_with(' ')).count();
        eprintln!("      {} top-level folder(s) found", count);
    }

    // ── real-debrid downloads ─────────────────────────────────────────────────
    eprintln!("[3/5] Fetching downloads from real-debrid...");
    let downloads = debrid::list_downloads(&debrid_token).await?;
    eprintln!("      {} downloads found", downloads.len());

    if downloads.is_empty() {
        eprintln!("No downloads found — nothing to do.");
        return Ok(());
    }

    // ── classify downloads ────────────────────────────────────────────────────
    eprintln!("[4/6] Classifying {} downloads with {}...", downloads.len(), ai::CLASSIFIER_MODEL);
    use futures::StreamExt as _;
    let results: Vec<_> = futures::stream::iter(downloads.iter().map(|d| {
        let key = openrouter_api_key.clone();
        let filename = d.filename.clone();
        let conv = conventions.to_string();
        async move {
            let messages = vec![
                ai::Message {
                    role: "system".to_string(),
                    content: "You are a media file classifier. Answer only 'yes' or 'no', nothing else.".to_string(),
                },
                ai::Message {
                    role: "user".to_string(),
                    content: format!(
                        "Given these media collection conventions:\n{conv}\n\n\
                         Does this file belong in this collection?\n\
                         Filename: {filename}\n\n\
                         Answer yes or no only."
                    ),
                },
            ];
            ai::chat_text(&key, ai::CLASSIFIER_MODEL, &messages).await
        }
    }))
    .buffer_unordered(10)
    .collect()
    .await;
    let total = results.len();
    let relevant: Vec<_> = downloads.into_iter().zip(results).filter_map(|(d, res)| {
        match res {
            Ok(ans) if ans.trim().to_lowercase().starts_with('y') => Some(d),
            Ok(_) => None,
            Err(e) => {
                eprintln!("      classify error for '{}': {e}", d.filename);
                None
            }
        }
    }).collect();

    eprintln!("      {}/{} downloads match these conventions", relevant.len(), total);

    if relevant.is_empty() {
        eprintln!("No matching downloads — nothing to do.");
        return Ok(());
    }

    // ── AI call ───────────────────────────────────────────────────────────────
    eprintln!("[5/6] Asking {} to create download.sh files...", model);
    let downloads_json = serde_json::to_string_pretty(&relevant)?;

    let existing_block = if existing_structure.is_empty() {
        String::new()
    } else {
        format!(
            "EXISTING STRUCTURE (already on disk — do NOT re-download these):\n\
             {existing_structure}\n\n"
        )
    };

    let system_msg = format!(
        "You are a media file organizer. \
         Given a list of real-debrid downloads and naming conventions, \
         you group the files into the correct folder structure \
         and emit the wget commands for each download.sh script.\n\n\
         CONVENTIONS (follow these exactly — they define folder structure, naming, and which media types to include):\n\
         {conventions}\n\n\
         {existing_block}\
         Respond with ONLY a JSON object matching this exact schema (no markdown):\n\
         {{\"scripts\":[{{\"path\":\"path/to/subfolder\",\
         \"content\":\"wget -O \\\"filename.ext\\\" \\\"https://...\\\"\\n\"}}]}}\n\n\
         Rules:\n\
         - path is relative to the output directory; derive the structure from CONVENTIONS\n\
         - Use the 'download' field from each entry as the wget URL\n\
         - Output filename must follow the naming rules in CONVENTIONS\n\
         - One wget -O line per file; use double-quotes around filename and URL\n\
         - Group files that belong in the same folder into the same script\n\
         - If you cannot determine where a file belongs from the filename, use best judgement\n\
         - If a folder already exists in EXISTING STRUCTURE, use the EXACT folder name shown above\n\
         - If a subfolder already exists in EXISTING STRUCTURE, skip all files for that subfolder (assume they are already downloaded)"
    );

    let user_msg = format!(
        "Here are my current real-debrid downloads:\n{downloads_json}\n\n\
         Please create the download.sh files."
    );

    let messages = vec![
        ai::Message { role: "system".to_string(), content: system_msg },
        ai::Message { role: "user".to_string(), content: user_msg },
    ];

    let response = ai::chat_with_fallback(&openrouter_api_key, model, &messages).await?;

    // ── parse response ────────────────────────────────────────────────────────
    #[derive(serde::Deserialize)]
    struct AiResponse {
        scripts: Vec<downloader::DownloadScript>,
    }

    let ai: AiResponse = serde_json::from_str(&response)
        .with_context(|| format!("AI returned invalid JSON:\n{response}"))?;

    eprintln!("      AI proposed {} script(s)", ai.scripts.len());

    // ── write scripts ─────────────────────────────────────────────────────────
    eprintln!("[6/6] Writing scripts to {}...", output_dir.display());
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
