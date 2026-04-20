# debrid-collector

CLI tool to organize and download Real-Debrid media files. Uses Groq AI to automatically generate folder-structured download scripts for TV shows, respecting your naming conventions.

## Prerequisites

- **[Real-Debrid](https://real-debrid.com)** account with API token
- **[Groq](https://console.groq.com)** API key (for AI mode)
- `fzf` (optional, for interactive model picker)

## Setup

Set your credentials via environment variables:

```sh
export REAL_DEBRID_API_TOKEN=your_token
export MEDIA_GROQ_API_KEY=your_groq_key
```

Or store them in the `kv` secret store if you have `kv_cli` installed.

## Usage

```
debrid-collector <COMMAND>
```

---

## AI Mode — `generate`

The primary workflow. Fetches your Real-Debrid downloads and uses Groq AI to organize them into season folders and generate executable download scripts.

```sh
debrid-collector generate [OPTIONS]
```

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-o, --output-dir <DIR>` | `.` | Where to write the generated folder structure |
| `--run` | — | Execute all generated scripts immediately |
| `--dry-run` | — | Preview what would be written without creating files |
| `-m, --model <MODEL>` | see below | Override the Groq model |

**Examples:**

```sh
# Preview AI-generated structure without writing anything
debrid-collector generate --dry-run

# Generate scripts into ~/Media
debrid-collector generate -o ~/Media

# Generate and immediately start downloading
debrid-collector generate -o ~/Media --run

# Use a specific model
debrid-collector generate -m llama-3.3-70b-versatile
```

**What it does:**

1. Reads `CONVENTIONS.md` for your naming rules
2. Fetches your current downloads from Real-Debrid
3. Sends everything to Groq AI, which groups files into `Show/Season/` folders
4. Writes a `download.sh` script in each season directory
5. Optionally runs the scripts (`--run`)

**Output structure example:**

```
~/Media/
  The Rookie [imdbid-tt7587890]/
    S03/
      download.sh   ← contains wget commands for each episode
  Breaking Bad [imdbid-tt0903747]/
    S01/
      download.sh
```

---

## Manual Mode — `run`

Execute an existing `download.sh` script manually, without any AI involvement.

```sh
debrid-collector run <DIR>
```

**Arguments:**

- `<DIR>` — path to the directory containing the `download.sh` script

**Examples:**

```sh
# Run a previously generated script
debrid-collector run ~/Media/The\ Rookie/S03

# Run a manually crafted script
debrid-collector run ./downloads/show-s01
```

---

## Torrent Management — `torrent`

Manage torrents on your Real-Debrid account.

### `torrent show`

List all completed torrents (100% progress):

```sh
debrid-collector torrent show
```

Output columns: `ID`, `FILENAME`, `SIZE`, `ADDED`

---

### `torrent download`

Unrestrict and print download links for torrent files:

```sh
debrid-collector torrent download [PATTERN]
```

- `PATTERN` — optional glob pattern to filter by filename (case-insensitive, `*` wildcard)
- Omit to process all completed torrents

```sh
# Get links for all completed torrents
debrid-collector torrent download

# Get links only for season 2 torrents
debrid-collector torrent download "S02*"
```

---

### `torrent delete`

Delete torrents from Real-Debrid:

```sh
debrid-collector torrent delete [PATTERN]
```

- `PATTERN` — optional glob pattern (case-insensitive)
- Omit to delete **all** completed torrents

```sh
# Delete a specific torrent by name pattern
debrid-collector torrent delete "Show.Name.S01*"

# Delete all completed torrents (use with caution)
debrid-collector torrent delete
```

---

## Model Selection — `models`

Interactively pick a Groq model and save it to `CONVENTIONS.md`:

```sh
debrid-collector models
```

Uses `fzf` if available, otherwise shows a numbered list. The selected model is saved to the YAML frontmatter of `CONVENTIONS.md` and will be used as the default for future `generate` runs.

**Model priority order:**

1. `-m` flag (per-run override)
2. `model:` field in `CONVENTIONS.md` frontmatter
3. Default: `qwen/qwen3-32b`
4. Automatic fallbacks if the model fails: `openai/gpt-oss-120b`, `llama-3.3-70b-versatile`

---

## Naming Conventions (`CONVENTIONS.md`)

Create a `CONVENTIONS.md` file in your working directory to guide the AI:

```markdown
---
model: qwen/qwen3-32b
---

- Seasons go in their own folder: `S01`, `S02`, etc.
- Filenames must include season and episode: `S01E01.mkv`
- Use the original language title
- Include IMDB ID when known: `[imdbid-tt7587890]`

Example: `The Rookie [imdbid-tt7587890]/S03/S03E01 - Consequences WEBRip-1080p.mkv`
```

The AI reads this file on every `generate` run to produce consistent, correctly named output.
