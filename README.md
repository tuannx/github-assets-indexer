# github-local-indexer

Local-first GitHub resource indexer — per-repo SQLite + FTS5, optimized for coding agents.

**Repository:** https://github.com/tuannx/github-assets-indexer

## Quick start

```bash
# 1. Install into your git repo (binary + skill + workspace)
cargo install --path .
cd your-git-repo
github-local-indexer install

# 2. Index a GitHub repo
export GITHUB_TOKEN=ghp_...
github-local-indexer repo add owner/name
github-local-indexer sync owner/name --wait

# 3. Search locally
github-local-indexer search "login bug" --json
```

Agents read `.github-local-indexer/INDEX.md` first — includes `last_indexed_at`, `index_age_minutes`, and `refresh_recommended` for cache decisions.

## Install options

| Method | Command |
|--------|---------|
| Per-repo (default) | `github-local-indexer install` |
| Global binary + skill | `github-local-indexer install --global` |
| Global skill only | `npx skills add tuannx/github-assets-indexer -g -a cursor -y --skill github-local-indexer` |

Per-repo install creates:

```text
.github-local-indexer/   INDEX.md, status.json, index.db
.cursor/bin/           github-local-indexer
.cursor/skills/        github-local-indexer/SKILL.md
.cursor/rules/         github-local-indexer.mdc
```

## Agent workflow

1. Read `.github-local-indexer/INDEX.md` and `status.json`
2. `github-local-indexer search "<query>" --json` (never calls GitHub)
3. `github-local-indexer sync owner/repo --wait` if stale or empty
4. Remote GitHub only when user explicitly asks

## Development

```bash
cargo test
cargo clippy -- -D warnings
cargo run -- doctor
```

## Docs

- [Implementation guide](./doc/IMPLEMENTATION-MINIMUM.md) — slices S0–S9 done, S10 planned
- [Requirements](./doc/design/github-local-indexer-solution-requirements.md)

## Status (v0.1)

| Feature | Status |
|---------|--------|
| Issues, comments, PRs, reviews, inline comments | Done |
| FTS5 local search | Done |
| Incremental sync + checkpoint overlap | Done |
| Per-repo workspace + INDEX.md for agents | Done |
| Job queue (`sync` / `jobs run`) | Done |
| Daemon / auto-poll | Planned (S10) |
| Projects v2, Wiki | Planned (S10) |
