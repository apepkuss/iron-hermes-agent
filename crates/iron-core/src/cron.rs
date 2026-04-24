//! Cron job data model and schedule parsing.

use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub model: Option<String>,
    pub disabled_toolsets: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub next_run_at: Option<String>,
    pub next_run_at_epoch: Option<i64>,
    pub last_run_at: Option<String>,
    pub running: bool,
    pub failure_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRun {
    pub id: i64,
    pub job_id: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub duration_ms: u64,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewCronJob {
    pub name: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub model: Option<String>,
    pub disabled_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CronJobPatch {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<String>,
    pub enabled: Option<bool>,
    pub model: Option<Option<String>>,
    pub disabled_toolsets: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct CronRunFinish {
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub duration_ms: u64,
    pub session_id: Option<String>,
}

pub fn next_run_epoch(schedule: &str, after_epoch: i64) -> Result<i64, CoreError> {
    let schedule = schedule.trim();
    if schedule.is_empty() {
        return Err(CoreError::Session("cron schedule is empty".to_string()));
    }

    let lower = schedule.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("every ") {
        let seconds = parse_duration_seconds(rest.trim())?;
        return Ok(after_epoch.saturating_add(seconds));
    }

    if let Some(rest) = lower.strip_prefix("daily ") {
        let (hour, minute) = parse_hhmm(rest.trim())?;
        return Ok(next_daily_epoch(after_epoch, hour, minute));
    }

    if schedule.split_whitespace().count() == 5 {
        return next_cron_epoch(schedule, after_epoch);
    }

    Err(CoreError::Session(format!(
        "unsupported cron schedule: {schedule}. Use 'every 30m', 'every 2h', 'daily 09:00', or a 5-field cron expression."
    )))
}

fn parse_duration_seconds(input: &str) -> Result<i64, CoreError> {
    let compact = input.replace(' ', "");
    if compact.len() < 2 {
        return Err(CoreError::Session(format!("invalid duration: {input}")));
    }
    let (num_part, unit_part) = compact.split_at(compact.len() - 1);
    let n: i64 = num_part
        .parse()
        .map_err(|_| CoreError::Session(format!("invalid duration: {input}")))?;
    if n <= 0 {
        return Err(CoreError::Session("duration must be positive".to_string()));
    }
    let mul = match unit_part {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(CoreError::Session(format!(
                "invalid duration unit in {input}; use s, m, h, or d"
            )));
        }
    };
    Ok(n.saturating_mul(mul))
}

fn parse_hhmm(input: &str) -> Result<(u32, u32), CoreError> {
    let Some((h, m)) = input.split_once(':') else {
        return Err(CoreError::Session(format!("invalid daily time: {input}")));
    };
    let hour: u32 = h
        .parse()
        .map_err(|_| CoreError::Session(format!("invalid hour: {h}")))?;
    let minute: u32 = m
        .parse()
        .map_err(|_| CoreError::Session(format!("invalid minute: {m}")))?;
    if hour > 23 || minute > 59 {
        return Err(CoreError::Session(format!("invalid daily time: {input}")));
    }
    Ok((hour, minute))
}

fn next_daily_epoch(after_epoch: i64, hour: u32, minute: u32) -> i64 {
    let day = after_epoch.div_euclid(86400);
    let target = day * 86400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
    if target > after_epoch {
        target
    } else {
        target + 86400
    }
}

fn next_cron_epoch(schedule: &str, after_epoch: i64) -> Result<i64, CoreError> {
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(CoreError::Session(
            "cron expression must have 5 fields".to_string(),
        ));
    }

    let mut candidate = ((after_epoch / 60) + 1) * 60;
    let end = after_epoch.saturating_add(366 * 86400);
    while candidate <= end {
        let parts = epoch_parts(candidate);
        if cron_field_matches(fields[0], parts.minute, 0, 59)?
            && cron_field_matches(fields[1], parts.hour, 0, 23)?
            && cron_field_matches(fields[2], parts.day, 1, 31)?
            && cron_field_matches(fields[3], parts.month, 1, 12)?
            && cron_field_matches(fields[4], parts.weekday, 0, 6)?
        {
            return Ok(candidate);
        }
        candidate += 60;
    }
    Err(CoreError::Session(format!(
        "cron expression has no run time within one year: {schedule}"
    )))
}

fn cron_field_matches(field: &str, value: u32, min: u32, max: u32) -> Result<bool, CoreError> {
    for part in field.split(',') {
        let part = part.trim();
        if part == "*" {
            return Ok(true);
        }
        if let Some(step) = part.strip_prefix("*/") {
            let step: u32 = step
                .parse()
                .map_err(|_| CoreError::Session(format!("invalid cron step: {part}")))?;
            if step == 0 {
                return Err(CoreError::Session("cron step must be positive".to_string()));
            }
            if (value - min).is_multiple_of(step) {
                return Ok(true);
            }
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_cron_num(start, min, max)?;
            let end = parse_cron_num(end, min, max)?;
            if start <= value && value <= end {
                return Ok(true);
            }
            continue;
        }
        let exact = parse_cron_num(part, min, max)?;
        if value == exact {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_cron_num(input: &str, min: u32, max: u32) -> Result<u32, CoreError> {
    let value: u32 = input
        .parse()
        .map_err(|_| CoreError::Session(format!("invalid cron field value: {input}")))?;
    if value < min || value > max {
        return Err(CoreError::Session(format!(
            "cron value {value} out of range {min}-{max}"
        )));
    }
    Ok(value)
}

struct EpochParts {
    minute: u32,
    hour: u32,
    day: u32,
    month: u32,
    weekday: u32,
}

fn epoch_parts(epoch: i64) -> EpochParts {
    let epoch = epoch.max(0) as u64;
    let minute = ((epoch / 60) % 60) as u32;
    let hour = ((epoch / 3600) % 24) as u32;
    let days = epoch / 86400;
    let (year, month, day) = days_to_ymd(days);
    let weekday = ((days + 4) % 7) as u32; // 1970-01-01 was Thursday.
    let _ = year;
    EpochParts {
        minute,
        hour,
        day: day as u32,
        month: month as u32,
        weekday,
    }
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    loop {
        let leap = is_leap(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: &[u64] = if leap {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1u64;
    for &md in month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(year: u64) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_schedule() {
        assert_eq!(next_run_epoch("every 5m", 100).unwrap(), 400);
        assert_eq!(next_run_epoch("every 2h", 100).unwrap(), 7300);
    }

    #[test]
    fn parses_daily_schedule() {
        assert_eq!(next_run_epoch("daily 00:01", 0).unwrap(), 60);
        assert_eq!(next_run_epoch("daily 00:01", 60).unwrap(), 86460);
    }

    #[test]
    fn parses_simple_cron_schedule() {
        assert_eq!(next_run_epoch("*/15 * * * *", 0).unwrap(), 900);
        assert_eq!(next_run_epoch("0 1 * * *", 0).unwrap(), 3600);
    }
}
