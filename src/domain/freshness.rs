use chrono::{DateTime, Utc};

use crate::domain::resource::Freshness;

pub const FRESHNESS_TTL_MINUTES: i64 = 15;

pub fn index_age_minutes(last_indexed_at: Option<&str>) -> Option<i64> {
    let parsed = parse_rfc3339(last_indexed_at?)?;
    let age = Utc::now().signed_duration_since(parsed);
    Some(age.num_minutes())
}

pub fn refresh_recommended(freshness: Freshness) -> bool {
    matches!(freshness, Freshness::NeverSynced | Freshness::Stale)
}

pub fn collection_freshness(last_success_at: Option<&str>) -> Freshness {
    let Some(ts) = last_success_at else {
        return Freshness::NeverSynced;
    };
    let Some(parsed) = parse_rfc3339(ts) else {
        return Freshness::Stale;
    };
    let age = Utc::now().signed_duration_since(parsed);
    if age.num_minutes() <= FRESHNESS_TTL_MINUTES {
        Freshness::Fresh
    } else {
        Freshness::Stale
    }
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_after_ttl() {
        let old = (Utc::now() - chrono::Duration::minutes(20)).to_rfc3339();
        assert_eq!(collection_freshness(Some(&old)), Freshness::Stale);
    }

    #[test]
    fn refresh_when_stale_or_never() {
        assert!(refresh_recommended(Freshness::Stale));
        assert!(refresh_recommended(Freshness::NeverSynced));
        assert!(!refresh_recommended(Freshness::Fresh));
    }
}
