use super::month_range;
use time::{OffsetDateTime, macros::datetime};

#[test]
fn month_ranges_include_the_last_millisecond_but_exclude_the_next_month() {
    for (year, month, start, end) in [
        (
            1,
            1,
            datetime!(0001-01-01 0:00 UTC),
            datetime!(0001-02-01 0:00 UTC),
        ),
        (
            99,
            12,
            datetime!(0099-12-01 0:00 UTC),
            datetime!(0100-01-01 0:00 UTC),
        ),
        (
            2023,
            2,
            datetime!(2023-02-01 0:00 UTC),
            datetime!(2023-03-01 0:00 UTC),
        ),
        (
            2024,
            2,
            datetime!(2024-02-01 0:00 UTC),
            datetime!(2024-03-01 0:00 UTC),
        ),
        (
            2026,
            12,
            datetime!(2026-12-01 0:00 UTC),
            datetime!(2027-01-01 0:00 UTC),
        ),
    ] {
        let millis = |date: OffsetDateTime| date.unix_timestamp() * 1000;
        let range = month_range(year, month).unwrap();
        assert_eq!(range.start, millis(start));
        assert_eq!(range.end, millis(end));
        assert!(!range.contains(&(millis(start) - 1)));
        assert!(range.contains(&millis(start)));
        assert!(range.contains(&(millis(end) - 1)));
        assert!(!range.contains(&millis(end)));
    }
    let last = month_range(9999, 12).unwrap();
    assert_eq!(last.start, 253_399_622_400_000);
    assert_eq!(last.end, 253_402_300_800_000);
    assert!(last.contains(&(last.end - 1)));
    assert!(!last.contains(&last.end));
    for (year, month) in [(0, 1), (10_000, 1), (1, 0), (1, 13)] {
        assert!(month_range(year, month).is_err());
    }
}
