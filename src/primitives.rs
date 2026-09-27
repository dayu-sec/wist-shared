//! 跨域共用的原始类型（模型标量 DateTime/Secret 的单一真源）。

pub type Int = i64;
pub type Bool = bool;
pub type Float = f64;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, ::serde::Serialize, ::serde::Deserialize,
)]
pub struct DateTime(chrono::DateTime<chrono::Utc>);

impl DateTime {
    pub fn now() -> Self {
        Self(chrono::Utc::now())
    }
    /// `days` 天后的时刻（如注册 Token 有效期）。
    ///
    /// `days` 越界（大到 `Duration` 或加法溢出）时退化为"现在"：与下面 `checked_add_signed`
    /// 的兜底一致，按已过期处理，而不是 panic。
    pub fn in_days(days: i64) -> Self {
        let now = chrono::Utc::now();
        let shifted =
            chrono::Duration::try_days(days).and_then(|delta| now.checked_add_signed(delta));
        Self(shifted.unwrap_or(now))
    }
    pub fn from_rfc3339(value: &str) -> Option<Self> {
        chrono::DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|v| Self(v.with_timezone(&chrono::Utc)))
    }
    /// 取底层 chrono 值（如 sqlx TIMESTAMPTZ 绑定）。
    pub fn to_chrono(&self) -> chrono::DateTime<chrono::Utc> {
        self.0
    }
    pub fn seconds_until(&self, later: &Self) -> i64 {
        later.0.signed_duration_since(self.0).num_seconds().max(0)
    }
}

#[derive(Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct Secret(String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_time_round_trips_through_serde_json() {
        let value = DateTime::from_rfc3339("2026-09-23T20:24:24Z").expect("parse");
        let json = serde_json::to_string(&value).expect("serialize");
        let back: DateTime = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(value, back);
        assert_eq!(back.to_chrono().to_rfc3339(), "2026-09-23T20:24:24+00:00");
    }

    #[test]
    fn from_rfc3339_rejects_garbage() {
        assert!(DateTime::from_rfc3339("not a timestamp").is_none());
        // 不带时区的朴素时间不是合法 RFC 3339。
        assert!(DateTime::from_rfc3339("2026-09-23 20:24:24").is_none());
    }

    #[test]
    fn in_days_moves_forward_and_backward() {
        let now = DateTime::now();
        assert!(DateTime::in_days(1).to_chrono() > now.to_chrono());
        assert!(DateTime::in_days(-1).to_chrono() < now.to_chrono());
    }

    #[test]
    fn in_days_does_not_panic_on_absurd_inputs() {
        // 旧实现 `Duration::days` 越界会 panic；现在应当退化而不是崩溃。
        for days in [i64::MAX, i64::MIN, i64::MAX / 2] {
            let _ = DateTime::in_days(days);
        }
    }

    #[test]
    fn seconds_until_clamps_negative_differences_to_zero() {
        let base = DateTime::from_rfc3339("2026-09-23T20:00:00Z").expect("base");
        let later = DateTime::from_rfc3339("2026-09-23T20:00:30Z").expect("later");
        assert_eq!(base.seconds_until(&later), 30);
        assert_eq!(later.seconds_until(&base), 0, "负差被夹到 0");
        assert_eq!(base.seconds_until(&base), 0);
    }

    #[test]
    fn secret_debug_redacts_the_value() {
        let secret: Secret = serde_json::from_str("\"super-secret-token\"").expect("secret");
        let debug = format!("{secret:?}");
        assert_eq!(debug, "Secret(***)");
        assert!(!debug.contains("super-secret-token"));
    }

    #[test]
    fn secret_round_trips_through_serde_json() {
        let secret: Secret = serde_json::from_str("\"s3cr3t\"").expect("secret");
        assert_eq!(
            serde_json::to_string(&secret).expect("serialize"),
            "\"s3cr3t\""
        );
    }
}
