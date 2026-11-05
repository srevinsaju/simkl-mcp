use jiff::civil::{Date, date};
use rstest::rstest;
use simkl_api::data::Month;

use super::calendar_files;

fn month(year: i16, month: u8) -> Option<(i16, u8)> {
    Some((year, month))
}

/// The rolling file covers yesterday to a month ahead; anything else reads
/// every month file the range `[from, until)` touches, with `until` itself
/// excluded.
#[rstest]
#[case::inside_rolling_window(date(2026, 10, 10), date(2026, 10, 17), vec![None])]
#[case::from_yesterday(date(2026, 10, 9), date(2026, 10, 16), vec![None])]
#[case::before_yesterday(date(2026, 10, 1), date(2026, 10, 8), vec![month(2026, 10)])]
#[case::past_a_month_ahead(
    date(2026, 11, 5),
    date(2026, 11, 12),
    vec![month(2026, 11)],
)]
#[case::until_is_exclusive(date(2026, 9, 25), date(2026, 10, 1), vec![month(2026, 9)])]
#[case::across_new_year(
    date(2026, 12, 28),
    date(2027, 1, 4),
    vec![month(2026, 12), month(2027, 1)],
)]
fn calendar_file_choice(
    #[case] from: Date,
    #[case] until: Date,
    #[case] expected: Vec<Option<(i16, u8)>>,
) {
    let today = date(2026, 10, 10);
    let files: Vec<_> = calendar_files(today, from, until)
        .into_iter()
        .map(|file| file.map(|Month { year, month }| (year, month)))
        .collect();
    assert_eq!(files, expected);
}
