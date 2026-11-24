use super::*;

#[test]
fn dates_round_trip_through_unix_days() {
    assert_eq!(
        Date::from_unix_days(0),
        Date {
            year: 1970,
            month: 1,
            day: 1,
            weekday: 4
        }
    );
    for days in [-1, 0, 11_016, 20_780, 20_781] {
        let date = Date::from_unix_days(days);
        assert_eq!(Date::unix_days(date.year, date.month, date.day), days);
    }
    assert_eq!(Date::from_unix_days(11_016).label(), "Tue, Feb 29, 2000");
    assert_eq!(Date::parse("2026-11-22").unwrap().weekday, 0);
    assert_eq!(Date::parse("2026-13-01"), None);
    assert_eq!(Date::parse("2026-11"), None);
}

#[test]
fn parses_a_bundled_snapshot() {
    let year = parse(include_str!("../../assets/snapshots/torvalds.json")).unwrap();
    assert_eq!(year.user, "torvalds");
    assert_eq!(year.offset, year.days[0].date.weekday as usize);
    assert!(year.days.windows(2).all(|w| w[0].date < w[1].date));
    assert_eq!(
        year.total,
        year.days.iter().map(|d| d.contributions).sum::<u32>()
    );
    let (week, weekday) = year.cell(year.days.len() - 1);
    assert_eq!(week + 1, year.weeks());
    assert_eq!(weekday, year.days.last().unwrap().date.weekday as usize);
}

fn year_of(counts: &[u32]) -> Year {
    // 2026-01-04 is a Sunday.
    let start = Date::unix_days(2026, 1, 4);
    let counts: Vec<(Date, u32)> = counts
        .iter()
        .enumerate()
        .map(|(i, &c)| (Date::from_unix_days(start + i as i64), c))
        .collect();
    classify("someone".into(), &counts)
}

#[test]
fn busier_days_get_higher_levels_and_grander_buildings() {
    let counts: Vec<u32> = (0..28).collect();
    let year = year_of(&counts);
    let levels: Vec<u8> = year.days.iter().map(|d| d.level).collect();
    assert!(levels.windows(2).all(|w| w[0] <= w[1]), "{levels:?}");
    assert_eq!((levels[0], levels[27]), (0, 4));
    for day in &year.days {
        let tier = match day.building {
            Building::Empty | Building::Sakura | Building::Pine | Building::Lantern => 0,
            Building::Machiya | Building::Farmhouse => 1,
            Building::Teahouse => 2,
            Building::MerchantHouse | Building::Storehouse => 3,
            Building::Inn => 4,
        };
        assert!(
            tier == day.level
                || (day.level >= 2 && day.building == Building::Machiya && tier == 1)
                || (day.level == 4 && day.building == Building::MerchantHouse),
            "{day:?}"
        );
    }
}

#[test]
fn landmarks_mark_the_busiest_day_week_and_longest_streak() {
    let year = year_of(&[0, 1, 2, 0, 9, 1, 1, 1, 1, 1, 1, 0, 9, 0]);
    assert_eq!(year.busiest_day, Some(4));
    assert_eq!(
        year.busiest_week,
        Some(Week {
            start: 0,
            contributions: 14
        })
    );
    assert_eq!(year.longest_streak, Some(Streak { start: 4, days: 7 }));
    let quiet = year_of(&[0, 0, 0]);
    assert_eq!(
        (quiet.busiest_day, quiet.busiest_week, quiet.longest_streak),
        (None, None, None)
    );
}

#[test]
fn a_calendar_starting_midweek_offsets_its_weeks() {
    let year = parse(
        r#"{"user":"u","total":3,"days":[{"date":"2026-11-25","count":1},{"date":"2026-11-24","count":0},{"date":"2026-11-26","count":0},{"date":"2026-11-27","count":0},{"date":"2026-11-28","count":0},{"date":"2026-11-29","count":2}]}"#,
    )
    .unwrap();
    assert_eq!(year.offset, 2);
    assert_eq!(year.days[0].date.short(), "Nov 24");
    assert_eq!(year.cell(5), (1, 0));
    assert_eq!(year.weeks(), 2);
    assert!(parse(r#"{"user":"u","days":[]}"#).is_err());
}
