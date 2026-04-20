# debrid-collector

CLI tool to organize and download Real-Debrid media files. Uses OpenRouter AI to automatically generate folder-structured download scripts for any media type (TV shows, movies, audiobooks, music, etc.), respecting your naming conventions.

## Prerequisites

- **[Real-Debrid](https://real-debrid.com)** account with API token
- **[OpenRouter](https://openrouter.ai)** API key (for AI mode)
- `fzf` (optional, for interactive model picker)

## Setup

Set your credentials via environment variables:

```sh
export REAL_DEBRID_API_TOKEN=your_token
export OPENROUTER_API_KEY=your_openrouter_key
```

Or store them in the `kv` secret store if you have `kv_cli` installed.

## Usage

```
debrid-collector <COMMAND>
```

---

## AI Mode — `generate`

The primary workflow. Fetches your Real-Debrid downloads and uses OpenRouter AI to organize them into the correct folder structure and generate executable download scripts.

```sh
debrid-collector generate [OPTIONS]
```

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-o, --output-dir <DIR>` | `.` | Where to write the generated folder structure |
| `--run` | — | Execute all generated scripts immediately |
| `--dry-run` | — | Preview what would be written without creating files |
| `-m, --model <MODEL>` | see below | Override the AI model |

**Examples:**

```sh
# Preview AI-generated structure without writing anything
debrid-collector generate --dry-run

# Generate scripts into ~/Media
debrid-collector generate -o ~/Media

# Generate and immediately start downloading
debrid-collector generate -o ~/Media --run

# Use a specific model
debrid-collector generate -m meta-llama/llama-3.3-70b-instruct
```

**What it does:**

1. Reads `CONVENTIONS.md` from `--output-dir` (falls back to CWD) for your naming and structure rules
2. Scans the output directory for existing folders to avoid re-downloading
3. Fetches your current downloads from Real-Debrid
4. Sends everything to the AI, which groups files per your conventions
5. Writes a `download.sh` script in each folder
6. Optionally runs the scripts (`--run`)

**Output structure example (TV shows):**

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

Interactively pick an OpenRouter model and save it to `CONVENTIONS.md`:

```sh
debrid-collector models
```

Uses `fzf` if available, otherwise shows a numbered list. The selected model is saved to the YAML frontmatter of `CONVENTIONS.md` and will be used as the default for future `generate` runs.

**Model priority order:**

1. `-m` flag (per-run override)
2. `model:` field in `CONVENTIONS.md` frontmatter
3. Default: `z-ai/glm-5-plus`
4. Automatic fallbacks if the model fails: `qwen/qwen3-32b`, `meta-llama/llama-3.3-70b-instruct`

---

## Naming Conventions (`CONVENTIONS.md`)

Create a `CONVENTIONS.md` file in your output directory (or CWD) to guide the AI. It defines folder structure, naming rules, and which media types to include — so it works for any media type, not just TV shows.

```markdown
---
model: z-ai/glm-5-plus
---

- Seasons go in their own folder: `S01`, `S02`, etc.
- Filenames must include season and episode: `S01E01.mkv`
- Use the original language title
- Include IMDB ID when known: `[imdbid-tt7587890]`

Example: `The Rookie [imdbid-tt7587890]/S03/S03E01 - Consequences WEBRip-1080p.mkv`
```

The AI reads this file on every `generate` run. If `CONVENTIONS.md` exists in the output directory, it takes precedence over the one in CWD.
