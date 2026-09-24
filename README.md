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

Agents read `.github-local-indexer/INDEX.md` first — it reports `last_indexed_at`, `index_age_minutes`, the freshness signal, pending jobs, and the 24-hour async auto-enqueue threshold. A queued job is not a completed refresh.

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
2. If any registered collection is more than 24 hours old, run `github-local-indexer sync owner/repo` without `--wait` when no source job is pending/running; report the stale collections and job ID. This queues work but does not itself run a worker.
3. `github-local-indexer search "<query>" --json` (local only; mention stale collection ages)
4. Only add/initialize an unindexed repo after the user requests or confirms it
5. Remote GitHub only when user explicitly asks

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
| Agent auto-enqueue after a collection is stale for 24 hours | Done (job worker still required) |
| Daemon / auto-poll | Planned (S10) |
| Projects v2, Wiki | Planned (S10) |
