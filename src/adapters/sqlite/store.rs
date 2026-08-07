use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use crate::adapters::sqlite::migrate::migrate;
use crate::application::ports::{ResourceStore, SourceStore, SyncStore};
use crate::domain::error::AppError;
use crate::domain::freshness::{collection_freshness, index_age_minutes, refresh_recommended};
use crate::domain::resource::{
    CollectionStatus, Freshness, JobState, ResourceSnapshot, ResourceType, SearchHit, SourceStatus,
    SyncJob,
};
use crate::domain::source::{Source, SourceId};
use crate::domain::time::now_iso;

#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    pub fn new(path: PathBuf) -> Self {
        let conn = Connection::open(&path).expect("open sqlite");
        conn.pragma_update(None, "journal_mode", "WAL")
            .expect("wal");
        conn.pragma_update(None, "foreign_keys", "ON")
            .expect("fk");
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .expect("busy");
        Self {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    async fn with_conn<F, T>(&self, f: F) -> Result<T, AppError>
    where
        F: FnOnce(&Connection) -> Result<T, AppError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.blocking_lock();
            f(&guard)
        })
        .await
        .map_err(AppError::storage)?
    }
}

#[async_trait]
impl SourceStore for SqliteStore {
    async fn migrate(&self) -> Result<i32, AppError> {
        self.with_conn(migrate).await
    }

    async fn insert_source(&self, source: &Source) -> Result<(), AppError> {
        let source = source.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO sources(id, provider, owner, repository, canonical_url, enabled, created_at, updated_at)
                 VALUES (?1, 'github', ?2, ?3, ?4, 1, ?5, ?6)",
                params![
                    source.id.as_str(),
                    source.owner,
                    source.repository,
                    source.canonical_url,
                    source.created_at,
                    source.updated_at,
                ],
            )
            .map_err(AppError::storage)?;
            Ok(())
        })
        .await
    }

    async fn list_sources(&self) -> Result<Vec<Source>, AppError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, owner, repository, canonical_url, enabled, created_at, updated_at
                     FROM sources ORDER BY owner, repository",
                )
                .map_err(AppError::storage)?;
            let mut rows = stmt.query([]).map_err(AppError::storage)?;
            let mut out = Vec::new();
            while let Some(row) = rows.next().map_err(AppError::storage)? {
                out.push(row_to_source(row)?);
            }
            Ok(out)
        })
        .await
    }

    async fn find_source(&self, owner: &str, repo: &str) -> Result<Option<Source>, AppError> {
        let owner = owner.to_string();
        let repo = repo.to_string();
        self.with_conn(move |conn| read_source(conn, &owner, &repo)).await
    }

    async fn find_source_by_id(&self, id: &str) -> Result<Option<Source>, AppError> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, owner, repository, canonical_url, enabled, created_at, updated_at
                     FROM sources WHERE id = ?1",
                )
                .map_err(AppError::storage)?;
            let mut rows = stmt.query(params![id]).map_err(AppError::storage)?;
            if let Some(row) = rows.next().map_err(AppError::storage)? {
                Ok(Some(row_to_source(row)?))
            } else {
                Ok(None)
            }
        })
        .await
    }
}

fn read_source(conn: &Connection, owner: &str, repo: &str) -> Result<Option<Source>, AppError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, owner, repository, canonical_url, enabled, created_at, updated_at
             FROM sources WHERE owner = ?1 AND repository = ?2",
        )
        .map_err(AppError::storage)?;
    let mut rows = stmt
        .query(params![owner, repo])
        .map_err(AppError::storage)?;
    if let Some(row) = rows.next().map_err(AppError::storage)? {
        Ok(Some(row_to_source(row)?))
    } else {
        Ok(None)
    }
}

fn row_to_source(row: &rusqlite::Row<'_>) -> Result<Source, AppError> {
    Ok(Source {
        id: SourceId(row.get(0).map_err(AppError::storage)?),
        owner: row.get(1).map_err(AppError::storage)?,
        repository: row.get(2).map_err(AppError::storage)?,
        canonical_url: row.get(3).map_err(AppError::storage)?,
        enabled: row.get::<_, i32>(4).map_err(AppError::storage)? == 1,
        created_at: row.get(5).map_err(AppError::storage)?,
        updated_at: row.get(6).map_err(AppError::storage)?,
    })
}

#[async_trait]
impl ResourceStore for SqliteStore {
    async fn upsert_batch(&self, snapshots: &[ResourceSnapshot]) -> Result<usize, AppError> {
        let snapshots = snapshots.to_vec();
        self.with_conn(move |conn| upsert_resources(conn, &snapshots)).await
    }

    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, AppError> {
        let query = query.to_string();
        self.with_conn(move |conn| search_resources(conn, &query, limit))
            .await
    }

    async fn count_by_source(&self, source_id: &str) -> Result<usize, AppError> {
        let source_id = source_id.to_string();
        self.with_conn(move |conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM resources WHERE source_id = ?1",
                    params![source_id],
                    |row| row.get(0),
                )
                .map_err(AppError::storage)?;
            Ok(count as usize)
        })
        .await
    }
}

fn upsert_resources(conn: &Connection, snapshots: &[ResourceSnapshot]) -> Result<usize, AppError> {
    let synced_at = now_iso();
    let tx = conn.unchecked_transaction().map_err(AppError::storage)?;
    let mut count = 0usize;
    for snap in snapshots {
        let id = snap.stable_id();
        let parent_id = snap.parent_stable_id();
        tx.execute(
            "INSERT INTO resources(id, source_id, resource_type, remote_id, parent_resource_id,
                canonical_url, title, body, remote_updated_at, local_synced_at, content_hash)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
             ON CONFLICT(source_id, resource_type, remote_id) DO UPDATE SET
                parent_resource_id=excluded.parent_resource_id,
                canonical_url=excluded.canonical_url,
                title=excluded.title,
                body=excluded.body,
                remote_updated_at=excluded.remote_updated_at,
                local_synced_at=excluded.local_synced_at,
                content_hash=excluded.content_hash",
            params![
                id,
                snap.source_id.as_str(),
                snap.resource_type.as_str(),
                snap.remote_id,
                parent_id,
                snap.canonical_url,
                snap.title,
                snap.body,
                snap.remote_updated_at,
                synced_at,
                snap.content_hash(),
            ],
        )
        .map_err(AppError::storage)?;
        tx.execute(
            "DELETE FROM fts_documents WHERE resource_id = ?1",
            params![id],
        )
        .map_err(AppError::storage)?;
        tx.execute(
            "INSERT INTO fts_documents(resource_id, source_id, resource_type, title, body)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                snap.source_id.as_str(),
                snap.resource_type.as_str(),
                snap.title,
                snap.body,
            ],
        )
        .map_err(AppError::storage)?;
        count += 1;
    }
    tx.commit().map_err(AppError::storage)?;
    Ok(count)
}

fn fts_query(raw: &str) -> String {
    let escaped = raw.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

fn search_resources(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchHit>, AppError> {
    let fts = fts_query(query);
    let sql = "
        SELECT r.id, r.resource_type, s.owner || '/' || s.repository AS slug,
               r.title, r.body, r.canonical_url, r.remote_updated_at, r.local_synced_at,
               p.title, p.canonical_url, bm25(fts_documents) AS rank
        FROM fts_documents
        JOIN resources r ON r.id = fts_documents.resource_id
        JOIN sources s ON s.id = r.source_id
        LEFT JOIN resources p ON p.id = r.parent_resource_id
        WHERE fts_documents MATCH ?1
        ORDER BY rank, r.remote_updated_at DESC
        LIMIT ?2";
    let mut stmt = conn.prepare(sql).map_err(AppError::storage)?;
    let rows = stmt
        .query_map(params![fts, limit], |row| {
            let rt: String = row.get(1)?;
            Ok(SearchHit {
                resource_id: row.get(0)?,
                resource_type: ResourceType::parse(&rt).unwrap_or(ResourceType::Issue),
                source_slug: row.get(2)?,
                title: row.get(3)?,
                body_excerpt: row.get::<_, Option<String>>(4)?,
                canonical_url: row.get(5)?,
                remote_updated_at: row.get(6)?,
                local_synced_at: row.get(7)?,
                parent_title: row.get(8)?,
                parent_url: row.get(9)?,
                rank: row.get(10)?,
            })
        })
        .map_err(AppError::storage)?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(AppError::storage)
}

#[async_trait]
impl SyncStore for SqliteStore {
    async fn enqueue_job(&self, job: &SyncJob) -> Result<(), AppError> {
        let job = job.clone();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO sync_jobs(id, source_id, collection, kind, state, available_at, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    job.id,
                    job.source_id.as_str(),
                    job.collection,
                    job.kind,
                    job.state.as_str(),
                    now_iso(),
                    now_iso(),
                ],
            )
            .map_err(AppError::storage)?;
            Ok(())
        })
        .await
    }

    async fn claim_next_job(&self) -> Result<Option<SyncJob>, AppError> {
        self.with_conn(|conn| {
            let tx = conn.unchecked_transaction().map_err(AppError::storage)?;
            let running: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM sync_jobs WHERE state = 'running'",
                    [],
                    |row| row.get(0),
                )
                .map_err(AppError::storage)?;
            if running > 0 {
                tx.commit().map_err(AppError::storage)?;
                return Ok(None);
            }
            let job: Option<(String, String, String, String)> = tx
                .query_row(
                    "SELECT id, source_id, collection, kind FROM sync_jobs
                     WHERE state = 'pending' AND available_at <= ?1
                     ORDER BY created_at LIMIT 1",
                    params![now_iso()],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .ok();
            let Some((id, source_id, collection, kind)) = job else {
                tx.commit().map_err(AppError::storage)?;
                return Ok(None);
            };
            tx.execute(
                "UPDATE sync_jobs SET state='running', started_at=?2 WHERE id=?1",
                params![id, now_iso()],
            )
            .map_err(AppError::storage)?;
            tx.commit().map_err(AppError::storage)?;
            Ok(Some(SyncJob {
                id,
                source_id: SourceId(source_id),
                collection,
                kind,
                state: JobState::Running,
            }))
        })
        .await
    }

    async fn finish_job(&self, job_id: &str, ok: bool, error: Option<&str>) -> Result<(), AppError> {
        let job_id = job_id.to_string();
        let error = error.map(str::to_string);
        let state = if ok { "succeeded" } else { "failed" };
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE sync_jobs SET state=?2, finished_at=?3, last_error=?4 WHERE id=?1",
                params![job_id, state, now_iso(), error],
            )
            .map_err(AppError::storage)?;
            Ok(())
        })
        .await
    }

    async fn pending_jobs_for_source(&self, source_id: &str) -> Result<usize, AppError> {
        let source_id = source_id.to_string();
        self.with_conn(move |conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sync_jobs WHERE source_id=?1 AND state IN ('pending','running')",
                    params![source_id],
                    |row| row.get(0),
                )
                .map_err(AppError::storage)?;
            Ok(count as usize)
        })
        .await
    }

    async fn get_checkpoint(&self, source_id: &str, collection: &str) -> Result<Option<String>, AppError> {
        let source_id = source_id.to_string();
        let collection = collection.to_string();
        self.with_conn(move |conn| {
            let value: Option<String> = conn
                .query_row(
                    "SELECT cursor_json FROM sync_collections WHERE source_id=?1 AND collection=?2",
                    params![source_id, collection],
                    |row| row.get(0),
                )
                .ok();
            Ok(value)
        })
        .await
    }

    async fn set_checkpoint(&self, source_id: &str, collection: &str, cursor: &str) -> Result<(), AppError> {
        let source_id = source_id.to_string();
        let collection = collection.to_string();
        let cursor = cursor.to_string();
        let now = now_iso();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO sync_collections(source_id, collection, cursor_json, last_success_at)
                 VALUES (?1,?2,?3,?4)
                 ON CONFLICT(source_id, collection) DO UPDATE SET
                    cursor_json=excluded.cursor_json,
                    last_success_at=excluded.last_success_at",
                params![source_id, collection, cursor, now],
            )
            .map_err(AppError::storage)?;
            Ok(())
        })
        .await
    }

    async fn last_success_at(&self, source_id: &str, collection: &str) -> Result<Option<String>, AppError> {
        let source_id = source_id.to_string();
        let collection = collection.to_string();
        self.with_conn(move |conn| {
            let value: Option<String> = conn
                .query_row(
                    "SELECT last_success_at FROM sync_collections WHERE source_id=?1 AND collection=?2",
                    params![source_id, collection],
                    |row| row.get(0),
                )
                .ok();
            Ok(value)
        })
        .await
    }

    async fn source_status(&self, source: &Source) -> Result<SourceStatus, AppError> {
        let source = source.clone();
        self.with_conn(move |conn| {
            let resource_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM resources WHERE source_id=?1",
                    params![source.id.as_str()],
                    |row| row.get(0),
                )
                .map_err(AppError::storage)?;
            let pending_jobs: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sync_jobs WHERE source_id=?1 AND state IN ('pending','running')",
                    params![source.id.as_str()],
                    |row| row.get(0),
                )
                .map_err(AppError::storage)?;
            let last_indexed_at: Option<String> = conn
                .query_row(
                    "SELECT MAX(local_synced_at) FROM resources WHERE source_id=?1",
                    params![source.id.as_str()],
                    |row| row.get(0),
                )
                .ok()
                .flatten();
            let mut collections = Vec::new();
            let mut last_success_at: Option<String> = None;
            for collection in ["issues", "pull_requests"] {
                let success: Option<String> = conn
                    .query_row(
                        "SELECT last_success_at FROM sync_collections WHERE source_id=?1 AND collection=?2",
                        params![source.id.as_str(), collection],
                        |row| row.get(0),
                    )
                    .ok();
                let freshness = collection_freshness(success.as_deref());
                let collection_indexed_at = success.clone().or_else(|| last_indexed_at.clone());
                let collection_age = index_age_minutes(collection_indexed_at.as_deref());
                if is_newer(&success, &last_success_at) {
                    last_success_at = success.clone();
                }
                collections.push(CollectionStatus {
                    collection: collection.to_string(),
                    last_success_at: success,
                    last_indexed_at: collection_indexed_at,
                    index_age_minutes: collection_age,
                    refresh_recommended: refresh_recommended(freshness),
                    freshness,
                });
            }
            let freshness = aggregate_freshness(&collections, resource_count);
            let index_age = index_age_minutes(last_indexed_at.as_deref());
            Ok(SourceStatus {
                slug: source.slug(),
                freshness,
                last_success_at,
                last_indexed_at,
                index_age_minutes: index_age,
                refresh_recommended: refresh_recommended(freshness),
                resource_count: resource_count as usize,
                pending_jobs: pending_jobs as usize,
                collections,
            })
        })
        .await
    }
}

fn aggregate_freshness(collections: &[CollectionStatus], resource_count: i64) -> Freshness {
    if resource_count == 0 {
        return Freshness::NeverSynced;
    }
    if collections.iter().any(|c| c.freshness == Freshness::Stale) {
        Freshness::Stale
    } else if collections.iter().all(|c| c.freshness == Freshness::Fresh) {
        Freshness::Fresh
    } else {
        Freshness::NeverSynced
    }
}

fn is_newer(candidate: &Option<String>, current: &Option<String>) -> bool {
    match (candidate, current) {
        (Some(_), None) => true,
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resource::ResourceType;
    use crate::domain::source::Source;
    use tempfile::tempdir;

    #[tokio::test]
    async fn source_roundtrip() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("test.db"));
        store.migrate().await.unwrap();
        let source = Source::new("acme".into(), "demo".into());
        store.insert_source(&source).await.unwrap();
        let found = store.find_source("acme", "demo").await.unwrap().unwrap();
        assert_eq!(found.slug(), "acme/demo");
    }

    #[tokio::test]
    async fn search_after_upsert() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("test.db"));
        store.migrate().await.unwrap();
        let source = Source::new("acme".into(), "demo".into());
        store.insert_source(&source).await.unwrap();
        let snap = ResourceSnapshot {
            source_id: source.id.clone(),
            resource_type: ResourceType::Issue,
            remote_id: "1".into(),
            parent_remote_id: None,
            parent_resource_type: None,
            canonical_url: "https://github.com/acme/demo/issues/1".into(),
            title: Some("login bug".into()),
            body: Some("oauth fails".into()),
            remote_updated_at: None,
        };
        store.upsert_batch(&[snap]).await.unwrap();
        let hits = store.search("login", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
    }
}
