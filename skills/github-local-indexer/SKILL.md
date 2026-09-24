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
| `auto_sync_after_minutes` | Age threshold for automatically enqueueing a refresh (default **1440**, 24 hours) |
| `freshness` | `fresh` \| `stale` \| `never_synced` |
| `refresh_recommended` | Freshness signal only; by itself it does not trigger an automatic sync |

**Decision tree:**

1. `never_synced` or no sources → ask before `repo add`; initialize only after the user requests or confirms it.
2. For each registered source, inspect `collections[]`; if any collection has `index_age_minutes > auto_sync_after_minutes` (1440 when the field is absent), and source `pending_jobs` is 0, run `sync owner/repo` **without** `--wait` before searching. This durably enqueues one source refresh and returns a job ID; report the stale collection names and queued job, then continue searching local data. The source-level age is the newest local write and can hide an older collection. The current CLI has no always-on daemon, so do not imply that a queued job is already being processed.
3. If any collection is over threshold and source `pending_jobs > 0`, do not enqueue a duplicate; report that work is already pending/running.
4. If collections are stale by their shorter freshness TTL but none is older than 24 hours, search locally and report collection ages. Do not auto-enqueue solely because `refresh_recommended` is true.
5. `freshness: fresh` → search locally; no sync needed.
6. If enqueue or its worker fails (for example, credentials or storage), keep using local results where possible, report the failure and age, and do not retry automatically again during the same task. Never claim the refresh completed until a successful job result is observed.

## Agent commands (index / cache)

Replace `gli` with the resolved binary path (see below).

```bash
# 1. Read current per-source and per-collection age, including active jobs
gli status --json
# → read status.json's auto_sync_after_minutes plus source collections[].index_age_minutes and pending_jobs

# 2. Refresh INDEX.md snapshot only (no GitHub network)
gli status
gli doctor

# 3. Enqueue async refresh (requires GITHUB_TOKEN when a worker processes it)
gli sync owner/repo

# 4. Explicit foreground refresh
gli sync owner/repo --wait

# 5. Process one queued job (foreground worker)
gli jobs run

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
5. Automatically enqueue a registered source only when any collection exceeds `auto_sync_after_minutes` (24 hours by default); do not wait for completion. Ask before adding a source. A direct user request to refresh overrides the age threshold.
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
