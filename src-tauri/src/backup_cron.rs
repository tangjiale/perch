#[cfg(test)]
use chrono::Timelike;
use chrono::{DateTime, Datelike, Duration, Local, LocalResult, TimeZone, Utc};

#[derive(Clone, Debug)]
pub struct CronSchedule {
    fields: [Vec<u32>; 5],
    day_any: bool,
    week_any: bool,
}
impl CronSchedule {
    pub fn parse(value: &str) -> Result<Self, String> {
        let parts: Vec<_> = value.split_whitespace().collect();
        if parts.len() != 5 {
            return Err("Cron 必须包含分钟、小时、日、月、星期五个字段".into());
        }
        let bounds = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];
        let mut fields: [Vec<u32>; 5] = Default::default();
        for (i, part) in parts.iter().enumerate() {
            for item in part.split(',') {
                let chunks: Vec<_> = item.split('/').collect();
                if chunks.len() > 2 {
                    return Err("Cron 步长格式错误".into());
                }
                let step = if chunks.len() == 2 {
                    number(chunks[1])?
                } else {
                    1
                };
                if step == 0 {
                    return Err("Cron 步长必须大于零".into());
                }
                let (min, max) = bounds[i];
                let (start, end) = if chunks[0] == "*" {
                    (min, max)
                } else if let Some((a, b)) = chunks[0].split_once('-') {
                    (number(a)?, number(b)?)
                } else {
                    let n = number(chunks[0])?;
                    (n, if chunks.len() == 2 { max } else { n })
                };
                if start < min || end > max || start > end {
                    return Err("Cron 字段超出有效范围".into());
                }
                for n in (start..=end).step_by(step as usize) {
                    fields[i].push(if i == 4 && n == 7 { 0 } else { n });
                }
            }
            fields[i].sort_unstable();
            fields[i].dedup();
        }
        Ok(Self {
            fields,
            day_any: parts[2].starts_with('*'),
            week_any: parts[4].starts_with('*'),
        })
    }
    #[cfg(test)]
    fn matches(&self, time: DateTime<Utc>) -> bool {
        let t = time.with_timezone(&Local);
        let day = self.fields[2].contains(&t.day());
        let week = self.fields[4].contains(&t.weekday().num_days_from_sunday());
        let date = if self.day_any || self.week_any {
            day && week
        } else {
            day || week
        };
        date && self.fields[0].contains(&t.minute())
            && self.fields[1].contains(&t.hour())
            && self.fields[3].contains(&t.month())
    }
    pub fn next_after(&self, after: DateTime<Utc>) -> Result<DateTime<Utc>, String> {
        let mut date = after.with_timezone(&Local).date_naive();
        // 按本地日历日期筛选，仅展开匹配日期的小时/分钟；保留夏令时两种映射。
        for _ in 0..(366 * 8) {
            let day = self.fields[2].contains(&date.day());
            let week = self.fields[4].contains(&date.weekday().num_days_from_sunday());
            let date_matches = if self.day_any || self.week_any {
                day && week
            } else {
                day || week
            };
            if self.fields[3].contains(&date.month()) && date_matches {
                let mut best = None;
                for hour in &self.fields[1] {
                    for minute in &self.fields[0] {
                        let naive = date.and_hms_opt(*hour, *minute, 0).ok_or("Cron 时间无效")?;
                        let candidates = match Local.from_local_datetime(&naive) {
                            LocalResult::Single(t) => vec![t.with_timezone(&Utc)],
                            LocalResult::Ambiguous(a, b) => {
                                vec![a.with_timezone(&Utc), b.with_timezone(&Utc)]
                            }
                            LocalResult::None => Vec::new(),
                        };
                        for t in candidates {
                            if t > after && best.is_none_or(|old| t < old) {
                                best = Some(t);
                            }
                        }
                    }
                }
                if let Some(t) = best {
                    return Ok(t);
                }
            }
            date = date
                .checked_add_signed(Duration::days(1))
                .ok_or("日期超出范围")?;
        }

        Err("Cron 在未来八年内没有可执行日期，请检查日期组合".into())
    }
}
fn number(value: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| "Cron 仅支持数字、星号、范围、列表和步长".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_and_impossible_dates() {
        assert!(CronSchedule::parse("* * * *").is_err());
        assert!(CronSchedule::parse("*/0 * * * *").is_err());
        assert!(CronSchedule::parse("60 * * * *").is_err());
        assert!(CronSchedule::parse("0 0 31 2 *")
            .unwrap()
            .next_after(Utc::now())
            .is_err());
    }
    #[test]
    fn lists_ranges_steps_and_sunday() {
        let c = CronSchedule::parse("0,15-45/15 */2 * * 0,7").unwrap();
        assert_eq!(c.fields[0], vec![0, 15, 30, 45]);
        assert_eq!(c.fields[4], vec![0]);
    }
    #[test]
    fn day_and_week_use_or() {
        let c = CronSchedule::parse("* * 1 * 1").unwrap();
        let monday = Local::now();
        let t = monday.with_day(1).unwrap().with_timezone(&Utc);
        assert!(c.matches(t));
    }
    #[test]
    fn next_is_strictly_after_and_minute_aligned() {
        let c = CronSchedule::parse("* * * * *").unwrap();
        let now = Utc::now();
        let next = c.next_after(now).unwrap();
        assert!(next > now);
        assert_eq!(next.second(), 0);
    }
    #[test]
    fn wildcard_step_day_uses_and_with_weekday() {
        let c = CronSchedule::parse("* * */2 * 1").unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 13)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let t = Local
            .from_local_datetime(&date)
            .unwrap()
            .with_timezone(&Utc);
        assert!(!c.matches(t)); // 奇数日，但不是星期一。
        let next = c.next_after(t).unwrap();
        assert_eq!(
            next.with_timezone(&Local).weekday().num_days_from_sunday(),
            1
        );
        assert_eq!(next.with_timezone(&Local).day() % 2, 1);
    }
}
