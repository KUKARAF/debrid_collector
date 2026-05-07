# debrid-collector — Media Conventions

Each media type has its own folder with a dedicated `CONVENTIONS.md`.
Run `debrid-collector generate` from inside the media folder you want to populate.

| Folder | scan_depth | Structure |
|--------|-----------|-----------|
| `Audiobooks/` | 3 | `Author/Series/Book/files` |
| `TvShows/` | 2 | `Show [imdbid]/S01/S01E01.mkv` |
| `Movies/` | 1 | `Title (Year) [imdbid]/Title.mkv` |
| `Music/` | 1 | `Artist/Artist - Track.mp3` |
| `Books/` | 2 | `Author/Book Title/file.epub` |
| `Comics/` | 2 | `Author or Series/Volume/files` |
| `Courses/` | 2 | `Course Title/Module/lessons` |
| `Podcasts/` | 1 | `Show Name/Episode.mp3` |
| `Blinkist/` | 1 | `Category/Author - Title.m4a` |
