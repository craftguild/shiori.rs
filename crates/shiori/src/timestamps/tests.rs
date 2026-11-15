use super::{Dates, UnsupportedDate};
use std::time::{Duration, SystemTime};

#[test]
fn filesystem_date_bounds_include_year_one_and_reject_year_zero_or_ten_thousand() {
    let epoch = SystemTime::UNIX_EPOCH;
    let first = epoch - Duration::from_secs(62_135_596_800);
    let last = epoch + Duration::from_secs(253_402_300_799);
    assert_eq!(
        Dates::system(first, last).unwrap().created,
        "0001-01-01T00:00:00.000Z"
    );
    assert_eq!(
        Dates::system(first, last).unwrap().updated,
        "9999-12-31T23:59:59.000Z"
    );
    for invalid in [
        first - Duration::from_secs(1),
        last + Duration::from_secs(1),
    ] {
        assert!(
            Dates::system(invalid, last)
                .unwrap_err()
                .is::<UnsupportedDate>()
        );
        assert!(
            Dates::system(first, invalid)
                .unwrap_err()
                .is::<UnsupportedDate>()
        );
    }
}
