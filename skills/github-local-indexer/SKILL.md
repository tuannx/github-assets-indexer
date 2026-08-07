---
name: github-local-indexer
description: >-
  Search and sync GitHub issues, PRs, comments, reviews, and inline review
  comments via local SQLite index. Use when the user wants local-first GitHub
  search, offline issue/PR lookup, index a repository, check sync status, refresh
  a stale cache, or find contributions without calling GitHub API directly.
---

# GitHub Local Indexer (Agent Skill)

## Priority read order

When exploring this repository, **always read local index data before GitHub remote**:

1. `.github-local-indexer/INDEX.md` — freshness, `last_indexed_at`, `refresh_recommended`
2. `.github-local-indexer/status.json` — machine-readable snapshot (`index_age_minutes`, `stale_ttl_minutes`)
3. `.github-local-indexer/manifest.json` — install paths

## Cache refresh decision

Read `status.json` (or INDEX.md table) per indexed repo:

| Field | Meaning |
|-------|---------|
| `last_indexed_at` | Newest local write time (MAX `local_synced_at`) |
| `index_age_minutes` | Minutes since `last_indexed_at` |
| `stale_ttl_minutes` | TTL at snapshot root (default **15**) |
| `freshness` | `fresh` \| `stale` \| `never_synced` |
| `refresh_recommended` | **`true` → run sync before trusting search** |

**Decision tree:**

1. `never_synced` or no sources → `repo add` + `sync --wait`
2. `refresh_recommended: true` or `freshness: stale` → `sync owner/repo --wait` (confirm unless user asked)
3. `freshness: fresh` → `search --json` only; do not call GitHub remote

## Agent commands (index / cache)

Replace `gli` with the resolved binary path (see below).

```bash
# 1. Check whether cache needs refresh
gli status --json
# → read sources[].last_indexed_at, index_age_minutes, refresh_recommended

# 2. Refresh INDEX.md snapshot only (no GitHub network)
gli status
gli doctor

# 3. Pull GitHub → local cache (requires GITHUB_TOKEN)
gli sync owner/repo --wait

# 4. Background sync job
gli sync owner/repo          # enqueue
gli jobs run                 # process one job

# 5. Register a repository
gli repo add owner/repo
gli repo list --json

# 6. Search local cache (never calls GitHub)
gli search "<query>" --json
```

## Rules

1. **Local-first** — `search` and `status` never call GitHub hidden.
2. **CLI only** — shell the binary; no reimplemented GitHub logic.
3. **JSON** — `--json` on `search`, `status`, `repo list`.
4. **Cite index age** — mention `last_indexed_at` and `index_age_minutes` when results may be stale.
5. **Confirm** before `repo add` / `sync` unless user asked to refresh cache.
6. **Token** — `GITHUB_TOKEN` for sync only; never log it.

## Binary

Resolve in order:

1. `BIN` file in this skill directory (written by per-repo `install`)
2. `.cursor/bin/github-local-indexer` in the git repo root
3. `github-local-indexer` on `PATH` (from `install --global` → `~/.local/bin`)

## Install

```bash
npx skills add tuannx/github-assets-indexer -g -a cursor -y --skill github-local-indexer
github-local-indexer install    # per git repo: workspace + INDEX.md
```

## Resource types

`issue`, `issue_comment`, `pull_request`, `pull_request_comment`,
`pull_request_review`, `pull_request_review_comment`
