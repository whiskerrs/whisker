//! A GitHub user's year of contributions and the town each day becomes.

use serde::Deserialize;

/// One day of the calendar, oldest first in [`Year::days`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub date: Date,
    pub contributions: u32,
    /// 0 for no contributions, then 1 to 4 by quartile of the year's active days.
    pub level: u8,
    pub building: Building,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Year {
    pub user: String,
    pub days: Vec<Day>,
    /// Weekday of `days[0]`, so day `i` sits in week `(i + offset) / 7`.
    pub offset: usize,
    pub total: u32,
    pub busiest_day: Option<usize>,
    /// First day of the busiest Sunday-to-Saturday week.
    pub busiest_week: Option<Week>,
    pub longest_streak: Option<Streak>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Week {
    pub start: usize,
    pub contributions: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Streak {
    pub start: usize,
    pub days: usize,
}

impl Year {
    /// A town with no lots yet, shown until the first calendar arrives.
    pub fn empty(user: String) -> Year {
        Year {
            user,
            days: Vec::new(),
            offset: 0,
            total: 0,
            busiest_day: None,
            busiest_week: None,
            longest_streak: None,
        }
    }

    pub fn weeks(&self) -> usize {
        (self.days.len() + self.offset).div_ceil(7)
    }

    /// The calendar column and row of day `index`.
    pub fn cell(&self, index: usize) -> (usize, usize) {
        let slot = index + self.offset;
        (slot / 7, slot % 7)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// 0 is Sunday.
    pub weekday: u32,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

impl Date {
    fn from_unix_days(days: i64) -> Date {
        // Howard Hinnant's civil_from_days.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
        Date {
            year,
            month,
            day,
            weekday: (days + 4).rem_euclid(7) as u32,
        }
    }

    fn unix_days(year: i32, month: u32, day: u32) -> i64 {
        let y = i64::from(year) - i64::from(month <= 2);
        let era = y.div_euclid(400);
        let yoe = y.rem_euclid(400);
        let mp = (i64::from(month) + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    fn parse(text: &str) -> Option<Date> {
        let mut parts = text.split('-').map(str::parse::<i64>);
        let (year, month, day) = (
            parts.next()?.ok()?,
            parts.next()?.ok()?,
            parts.next()?.ok()?,
        );
        if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        Some(Date::from_unix_days(Date::unix_days(
            year as i32,
            month as u32,
            day as u32,
        )))
    }

    pub fn month_name(&self) -> &'static str {
        MONTHS[self.month as usize - 1]
    }

    pub fn label(&self) -> String {
        const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        format!(
            "{}, {} {}, {}",
            WEEKDAYS[self.weekday as usize],
            self.month_name(),
            self.day,
            self.year
        )
    }

    pub fn short(&self) -> String {
        format!("{} {}", self.month_name(), self.day)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Building {
    Empty,
    Sakura,
    Pine,
    Lantern,
    Machiya,
    Farmhouse,
    Teahouse,
    MerchantHouse,
    Storehouse,
    Inn,
}

impl Building {
    pub fn name(self) -> &'static str {
        match self {
            Building::Empty => "Empty lot",
            Building::Sakura => "Cherry tree",
            Building::Pine => "Black pine",
            Building::Lantern => "Stone lantern",
            Building::Machiya => "Townhouse",
            Building::Farmhouse => "Thatched farmhouse",
            Building::Teahouse => "Tea house",
            Building::MerchantHouse => "Merchant house",
            Building::Storehouse => "Storehouse",
            Building::Inn => "Inn",
        }
    }
}

#[derive(Deserialize)]
struct Calendar {
    user: String,
    days: Vec<CalendarDay>,
}

#[derive(Deserialize)]
struct CalendarDay {
    date: String,
    count: u32,
}

/// Parses the `{user, total, days: [{date, count}]}` calendar whisker.rs serves.
pub fn parse(json: &str) -> Result<Year, String> {
    let calendar: Calendar = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let mut counts = calendar
        .days
        .iter()
        .map(|day| {
            Date::parse(&day.date)
                .map(|date| (date, day.count))
                .ok_or_else(|| format!("bad date {}", day.date))
        })
        .collect::<Result<Vec<_>, _>>()?;
    counts.sort_by_key(|&(date, _)| date);
    if counts.is_empty() {
        return Err("The calendar has no days".into());
    }
    Ok(classify(calendar.user, &counts))
}

/// Ranks each day against the year's active days, the way GitHub shades its calendar.
pub fn classify(user: String, counts: &[(Date, u32)]) -> Year {
    let mut active: Vec<u32> = counts.iter().map(|&(_, c)| c).filter(|&c| c > 0).collect();
    active.sort_unstable();
    let quantile = |q: f32| -> u32 {
        if active.is_empty() {
            return 0;
        }
        active[((active.len() - 1) as f32 * q).round() as usize]
    };
    let (q1, q2, q3) = (quantile(0.25), quantile(0.5), quantile(0.75));
    let days: Vec<Day> = counts
        .iter()
        .map(|&(date, contributions)| {
            let level = match contributions {
                0 => 0,
                c if c <= q1 => 1,
                c if c <= q2 => 2,
                c if c <= q3 => 3,
                _ => 4,
            };
            let pick = |options: &[Building]| options[variant(date) % options.len()];
            let building = match level {
                0 => pick(&[
                    Building::Empty,
                    Building::Empty,
                    Building::Sakura,
                    Building::Empty,
                    Building::Pine,
                    Building::Empty,
                    Building::Sakura,
                    Building::Lantern,
                    Building::Empty,
                    Building::Pine,
                ]),
                1 => pick(&[Building::Machiya, Building::Farmhouse, Building::Machiya]),
                2 => pick(&[Building::Machiya, Building::Teahouse]),
                3 => pick(&[Building::MerchantHouse, Building::Storehouse]),
                _ => pick(&[Building::Inn, Building::MerchantHouse]),
            };
            Day {
                date,
                contributions,
                level,
                building,
            }
        })
        .collect();
    let offset = days[0].date.weekday as usize;

    let busiest_day = days
        .iter()
        .enumerate()
        .filter(|(_, d)| d.contributions > 0)
        .max_by(|(ia, a), (ib, b)| a.contributions.cmp(&b.contributions).then(ib.cmp(ia)))
        .map(|(i, _)| i);

    let mut busiest_week: Option<Week> = None;
    let mut start = 0;
    while start < days.len() {
        let end = (start + 7 - days[start].date.weekday as usize).min(days.len());
        let contributions = days[start..end].iter().map(|d| d.contributions).sum();
        if contributions > 0 && busiest_week.is_none_or(|w| contributions > w.contributions) {
            busiest_week = Some(Week {
                start,
                contributions,
            });
        }
        start = end;
    }

    let mut longest_streak: Option<Streak> = None;
    let mut run = 0;
    for (i, day) in days.iter().enumerate() {
        run = if day.contributions > 0 { run + 1 } else { 0 };
        if run > 0 && longest_streak.is_none_or(|s| run > s.days) {
            longest_streak = Some(Streak {
                start: i + 1 - run,
                days: run,
            });
        }
    }

    Year {
        user,
        total: days.iter().map(|d| d.contributions).sum(),
        days,
        offset,
        busiest_day,
        busiest_week,
        longest_streak,
    }
}

pub fn variant(date: Date) -> usize {
    let mut h = (date.year as u64) << 9 | (date.month as u64) << 5 | date.day as u64;
    h ^= h >> 7;
    h = h.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    (h >> 32) as usize
}

#[cfg(test)]
mod tests;
