use chrono::{DateTime, Duration, Utc};

use crate::domain::resource::ResourceSnapshot;

pub const CHECKPOINT_OVERLAP_SECS: i64 = 300;

pub fn max_remote_updated_at(snapshots: &[ResourceSnapshot]) -> Option<String> {
    snapshots
        .iter()
        .filter_map(|s| s.remote_updated_at.as_deref())
        .filter_map(parse_rfc3339)
        .max()
        .map(|dt| dt.to_rfc3339())
}

pub fn collection_checkpoint(
    parents: &[ResourceSnapshot],
    children: &[ResourceSnapshot],
    previous: Option<&str>,
) -> String {
    let mut combined = Vec::with_capacity(parents.len() + children.len());
    combined.extend_from_slice(parents);
    combined.extend_from_slice(children);

    if let Some(max_ts) = max_remote_updated_at(&combined) {
        return apply_overlap(&max_ts).unwrap_or(max_ts);
    }

    previous
        .map(str::to_string)
        .unwrap_or_else(crate::domain::time::now_iso)
}

fn apply_overlap(iso: &str) -> Option<String> {
    let parsed = parse_rfc3339(iso)?;
    Some((parsed - Duration::seconds(CHECKPOINT_OVERLAP_SECS)).to_rfc3339())
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resource::ResourceType;
    use crate::domain::source::SourceId;

    fn snap(updated: &str) -> ResourceSnapshot {
        ResourceSnapshot {
            source_id: SourceId::new(),
            resource_type: ResourceType::Issue,
            remote_id: "1".into(),
            parent_remote_id: None,
            parent_resource_type: None,
            canonical_url: "https://example.com".into(),
            title: None,
            body: None,
            remote_updated_at: Some(updated.into()),
        }
    }

    #[test]
    fn picks_latest_timestamp() {
        let snaps = vec![
            snap("2024-01-01T10:00:00Z"),
            snap("2024-01-02T12:00:00Z"),
        ];
        let max = max_remote_updated_at(&snaps).unwrap();
        let parsed = parse_rfc3339(&max).unwrap();
        assert_eq!(
            parsed,
            parse_rfc3339("2024-01-02T12:00:00Z").unwrap()
        );
    }

    #[test]
    fn overlap_moves_cursor_back() {
        let parents = vec![snap("2024-06-01T12:00:00Z")];
        let cursor = collection_checkpoint(&parents, &[], None);
        let parsed = parse_rfc3339(&cursor).unwrap();
        let expected = parse_rfc3339("2024-06-01T12:00:00Z").unwrap()
            - Duration::seconds(CHECKPOINT_OVERLAP_SECS);
        assert_eq!(parsed, expected);
    }

    #[test]
    fn empty_batch_keeps_previous() {
        let cursor = collection_checkpoint(&[], &[], Some("2024-01-01T00:00:00Z"));
        assert_eq!(cursor, "2024-01-01T00:00:00Z");
    }
}
