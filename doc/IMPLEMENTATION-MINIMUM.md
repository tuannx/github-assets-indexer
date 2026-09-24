# GitHub Local Indexer — Minimum Features Implementation Guide

> Phiên bản: 0.2  
> Tham chiếu: [github-local-indexer-solution-requirements.md](./design/github-local-indexer-solution-requirements.md)  
> Nguyên tắc: **mỗi slice phải compile, test và chạy được độc lập** trước khi sang slice tiếp theo.

## Mục tiêu của guide này

Tài liệu requirements đầy đủ (~1300 dòng) mô tả MVP hoàn chỉnh. Guide này cắt xuống **đường đi ngắn nhất** để có một binary Rust chạy được, lưu được source, index được issue/PR, search được local — rồi mở rộng dần.

**Đã hoàn thành (S0–S9 + agent workspace):**

- Issues, comments, PRs, PR comments, reviews, inline review comments
- Per-repo workspace `.github-local-indexer/` + agent skill/rule
- Checkpoint incremental với overlap window
- Child sync chỉ cho parent trả về trong batch incremental

**Chưa làm (S10+):**

- GitHub Projects v2 (GraphQL), Wiki git adapter
- Daemon `run`, lease recovery, retry/backoff đầy đủ
- Freshness/sync_health model đầy đủ (failure, confirmed_stale)
- Remote fallback search

---

## Decision gates (chốt)

| Gate | Quyết định |
|------|-----------|
| Runtime | Rust stable, `tokio` multi-thread |
| Storage | **Per-repo** SQLite tại `.github-local-indexer/index.db`; global `~/.local/share/...` chỉ khi `GITHUB_LOCAL_INDEXER_GLOBAL=1` |
| Agent integration | `INDEX.md` + `status.json` + Cursor skill/rule; refresh sau mọi mutation |
| Execution | `sync --wait` foreground; job queue + `jobs run`; daemon ở S10 |
| Credential | `GITHUB_TOKEN` env var only (keychain sau) |
| Provider | Fake provider trong test; GitHub REST từ S4 |
| Architecture | `domain` → `application` (ports) → `adapters` → `cli` |
| Incremental | Issues: `since` param; PRs: `sort=updated&direction=desc` + page cutoff (no `since` on `/pulls`) |

---

## Cấu trúc crate

```text
src/
  main.rs
  lib.rs
  cli/
  domain/                 # + project.rs, sync_cursor.rs
  application/            # + artifacts.rs, install.rs, bootstrap.rs
  adapters/
    sqlite/
    github/
    fake/
```

**Luật dependency:** `cli → application → domain`; `adapters` implement port của `application`.

**Per-repo workspace (sau `install`):**

```text
.github-local-indexer/
  INDEX.md          ← agent đọc TRƯỚC
  status.json
  manifest.json
  config.json
  index.db          ← gitignored
.cursor/
  bin/github-local-indexer
  skills/github-local-indexer/
  rules/github-local-indexer.mdc
```

---

## Slice roadmap

| Slice | Trạng thái | Nội dung |
|-------|-----------|----------|
| S0 | ✅ | CLI skeleton + `doctor` |
| S1 | ✅ | SQLite bootstrap + migration |
| S2 | ✅ | `repo add` / `repo list` |
| S3 | ✅ | Resources + FTS5 + fake ingest |
| S4 | ✅ | GitHub REST initial sync issues |
| S5 | ✅ | Issue comments + parent context |
| S6 | ✅ | Durable job queue (minimal) |
| S7 | ✅ | Checkpoint incremental + overlap |
| S8 | ✅ | `status` + freshness TTL 15 phút |
| S9 | ✅ | PRs + all PR contribution types |
| S9b | ✅ | `install`, agent skill, INDEX.md artifacts |
| S10 | 🔲 | Projects, Wiki, daemon, full freshness |

---

## Agent workflow (mặc định sau install)

1. Đọc `.github-local-indexer/INDEX.md` và `status.json`
2. Đọc tuổi từng collection vì source-level age là lần ghi local mới nhất và có thể che collection cũ
3. Nếu collection nào vượt `auto_sync_after_minutes` (mặc định 1440) và source chưa có job pending/running, gọi `sync owner/repo` không có `--wait`; báo collection stale và job ID
4. `search "<query>" --json` (local only), kể cả khi refresh vừa được enqueue
5. Nếu chỉ stale theo TTL ngắn hơn hoặc đã có job, nêu tuổi/trạng thái và không enqueue trùng
6. Chỉ add và initial-sync repo chưa đăng ký khi user yêu cầu hoặc xác nhận
7. Job được enqueue chưa đồng nghĩa cache đã refresh; chỉ báo hoàn tất sau khi worker trả kết quả thành công
8. Remote GitHub chỉ khi user yêu cầu

CLI **yêu cầu workspace** (config.json trong cây thư mục hiện tại). Nếu chưa install:

```text
no project workspace found — run `github-local-indexer install` inside a git repo
```

Global DB fallback: `export GITHUB_LOCAL_INDEXER_GLOBAL=1`

---

## Quy tắc viết code mỗi slice

1. **Một PR / một slice** — không nhảy slice.
2. **Test trước khi thêm dependency** — unit domain + integration SQLite.
3. **Không leak DTO** — GitHub struct chỉ ở `adapters/github`.
4. **Transaction ngắn** — upsert resource + fts cùng transaction.
5. **Không `unwrap()` trong production path** — `Result` + `thiserror`.
6. **CLI không gọi SQLite trực tiếp** — luôn qua application service.

---

## Lệnh dev chuẩn

```bash
cargo test
cargo fmt
cargo clippy -- -D warnings
cargo run -- doctor

# Cài vào repo hiện tại
cargo run -- install

# Trong consumer repo
.cursor/bin/github-local-indexer search "keyword" --json
```

---

## Mapping slice → requirements

| Slice | Requirements coverage |
|-------|----------------------|
| S0 | CLI skeleton, NFR-005 |
| S1 | §8 schema, migration |
| S2 | FR-001, FR-002, UC-001 (partial) |
| S3 | FR-017, FR-018, FR-020 (local search) |
| S4 | FR-005, G-001 (issues only) |
| S5 | §5.2 Issues comments |
| S6 | FR-012 (minimal queue) |
| S7 | FR-006, G-003 |
| S8 | G-004 (simplified) |
| S9 | §5.2 Pull requests + reviews |
| S9b | Skill interface P0, local-first agent |

---

## Bước tiếp theo

**Slice S10** theo thứ tự:

1. `daemon run` + lease recovery
2. Projects v2 (GraphQL)
3. Wiki git adapter
4. Full freshness/sync_health model
5. Remote fallback (explicit opt-in)
