use rusqlite::Connection;

use crate::domain::error::AppError;

const MIGRATIONS: &[(&str, i32)] = &[
    (include_str!("../../../migrations/001_init.sql"), 1),
    (include_str!("../../../migrations/002_resources.sql"), 2),
    (include_str!("../../../migrations/003_sync.sql"), 3),
];

pub(crate) fn migrate(conn: &Connection) -> Result<i32, AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )
    .map_err(AppError::storage)?;

    let current: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(AppError::storage)?;

    let mut version = current;
    for (sql, v) in MIGRATIONS {
        if *v <= current {
            continue;
        }
        conn.execute_batch(sql).map_err(AppError::storage)?;
        conn.execute(
            "INSERT INTO schema_migrations(version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![v, crate::domain::time::now_iso()],
        )
        .map_err(AppError::storage)?;
        version = *v;
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        let v1 = migrate(&conn).unwrap();
        let v2 = migrate(&conn).unwrap();
        assert_eq!(v1, 3);
        assert_eq!(v2, 3);
    }
}
