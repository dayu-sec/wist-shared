//! Shared time helpers.

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// 当前 UTC 时刻的 RFC 3339 字符串（如 `2026-09-23T20:24:24Z`）。
pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("format RFC3339 timestamp")
}

/// 当前 Unix 时间戳（毫秒）。
pub fn now_ts_ms() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000
}

/// `millis` 毫秒之后的 RFC 3339 时刻。
///
/// 用 `saturating_add` 而不是 `+`：`+` 在越界时会 panic，而 `millis` 是 `u64`，可能来自外部
/// 配置（如计划的 `max_total_duration_ms`）。超过 `i64` 毫秒表示范围时按 `i64::MAX` 处理，
/// 最终饱和到能表示的最大时刻，绝不 panic。
pub fn after_millis_rfc3339(millis: u64) -> String {
    let delta = time::Duration::milliseconds(i64::try_from(millis).unwrap_or(i64::MAX));
    OffsetDateTime::now_utc()
        .saturating_add(delta)
        .format(&Rfc3339)
        .expect("format RFC3339 timestamp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> OffsetDateTime {
        OffsetDateTime::parse(value, &Rfc3339).expect("parse rfc3339")
    }

    fn to_ms(value: OffsetDateTime) -> i128 {
        value.unix_timestamp_nanos() / 1_000_000
    }

    #[test]
    fn now_rfc3339_is_parseable_and_close_to_now() {
        let before = OffsetDateTime::now_utc();
        let parsed = parse(&now_rfc3339());
        let after = OffsetDateTime::now_utc();
        assert!(parsed >= before - time::Duration::seconds(1));
        assert!(parsed <= after + time::Duration::seconds(1));
    }

    #[test]
    fn now_ts_ms_is_a_plausible_epoch_millisecond() {
        // 2020-01-01 至 2100-01-01 的窗口：宽松，但能抓住秒/微秒/纳秒量级错误。
        let ms = now_ts_ms();
        assert!(ms > 1_577_836_800_000, "看起来不是毫秒时间戳: {ms}");
        assert!(ms < 4_102_444_800_000, "看起来不是毫秒时间戳: {ms}");
    }

    #[test]
    fn after_millis_zero_is_now_and_positive_moves_forward() {
        let start = to_ms(OffsetDateTime::now_utc());

        let at_zero = to_ms(parse(&after_millis_rfc3339(0)));
        assert!(
            (at_zero - start).abs() < 2_000,
            "偏离过大: {at_zero} vs {start}"
        );

        let plus = to_ms(parse(&after_millis_rfc3339(60_000)));
        assert!(
            (plus - start - 60_000).abs() < 2_000,
            "应大约晚 60s: {plus} vs {start}"
        );
    }

    #[test]
    fn after_millis_saturates_instead_of_panicking_on_huge_inputs() {
        // 旧实现用 `+`：越界会 panic，且 `millis as i64` 对 u64::MAX 会回绕成负数。
        // 现在应当饱和到一个未来时刻，且始终是可格式化的 RFC 3339。
        for millis in [u64::MAX, i64::MAX as u64, (i64::MAX as u64) + 1, 1u64 << 62] {
            let text = after_millis_rfc3339(millis);
            let parsed = parse(&text);
            assert!(
                parsed >= OffsetDateTime::now_utc(),
                "{millis} 应装出未来时刻，实际 {text}"
            );
        }
    }
}
