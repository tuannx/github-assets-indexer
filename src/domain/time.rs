use chrono::Utc;

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}
