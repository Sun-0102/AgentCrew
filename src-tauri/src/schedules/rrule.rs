use super::*;

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LocalDateTime {
    pub(super) year: i32,
    pub(super) month: u32,   // 1..=12
    pub(super) day: u32,     // 1..=31
    pub(super) hour: u32,    // 0..=23
    pub(super) minute: u32,  // 0..=59
    pub(super) weekday: u32, // 1=Mon .. 7=Sun
}

#[cfg(unix)]
pub(super) fn epoch_to_local(ts: i64) -> Option<LocalDateTime> {
    let t = ts as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let res = unsafe { libc::localtime_r(&t, &mut tm) };
    if res.is_null() {
        return None;
    }
    let weekday = if tm.tm_wday == 0 {
        7
    } else {
        tm.tm_wday as u32
    };
    Some(LocalDateTime {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        weekday,
    })
}

#[cfg(unix)]
fn local_to_epoch(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<i64> {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month as i32 - 1;
    tm.tm_mday = day as i32;
    tm.tm_hour = hour as i32;
    tm.tm_min = minute as i32;
    tm.tm_sec = 0;
    tm.tm_isdst = -1;
    let res = unsafe { libc::mktime(&mut tm) };
    if res == -1 {
        return None;
    }
    Some(res as i64)
}

#[cfg(not(unix))]
pub(super) fn epoch_to_local(ts: i64) -> Option<LocalDateTime> {
    // Civil date from Unix epoch (UTC fallback on non-Unix)
    let days = ts.div_euclid(86_400);
    let secs = ts.rem_euclid(86_400);
    let hour = (secs / 3600) as u32;
    let minute = ((secs % 3600) / 60) as u32;
    // 1970-01-01 was Thursday (4)
    let weekday = (((days + 3).rem_euclid(7)) + 1) as u32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
    Some(LocalDateTime {
        year,
        month: m,
        day: d,
        hour,
        minute,
        weekday,
    })
}

#[cfg(not(unix))]
fn local_to_epoch(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<i64> {
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let m = month;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    Some(days * 86_400 + hour as i64 * 3600 + minute as i64 * 60)
}

pub(super) fn parse_time_of_day(raw: &str) -> Result<(u32, u32), String> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let hour: u32 = parts
        .next()
        .ok_or("时间格式需为 HH:MM")?
        .parse()
        .map_err(|_| "时间格式需为 HH:MM")?;
    let minute: u32 = parts
        .next()
        .ok_or("时间格式需为 HH:MM")?
        .parse()
        .map_err(|_| "时间格式需为 HH:MM")?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return Err("时间需在 00:00 到 23:59 之间。".into());
    }
    Ok((hour, minute))
}

pub(super) fn parse_weekdays(raw: &str) -> Result<Vec<u32>, String> {
    let mut days = Vec::new();
    for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let day: u32 = token
            .parse()
            .map_err(|_| "星期格式无效，需为 1–7（周一至周日）。")?;
        if !(1..=7).contains(&day) {
            return Err("星期需在 1–7（周一至周日）之间。".into());
        }
        if !days.contains(&day) {
            days.push(day);
        }
    }
    if days.is_empty() {
        return Err("请至少选择一天执行。".into());
    }
    days.sort_unstable();
    Ok(days)
}

pub(super) fn parse_cron_field(field: &str, min: u32, max: u32, allow_sunday_seven: bool) -> Result<HashSet<u32>, String> {
    let mut set = HashSet::new();
    for part in field.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return Err("Cron 表达式包含空段。".into());
        }
        let (range_part, step) = match part.split_once('/') {
            Some((lhs, rhs)) => {
                let s: u32 = rhs.parse().map_err(|_| "Cron 步长无效。")?;
                if s == 0 {
                    return Err("Cron 步长不能为 0。".into());
                }
                (lhs, s)
            }
            None => (part, 1),
        };
        let (start, end) = if range_part == "*" {
            (min, max)
        } else if let Some((a, b)) = range_part.split_once('-') {
            let a: u32 = a.parse().map_err(|_| "Cron 范围起始值无效。")?;
            let b: u32 = b.parse().map_err(|_| "Cron 范围结束值无效。")?;
            if a > b {
                return Err("Cron 范围起始值不能大于结束值。".into());
            }
            (a, b)
        } else {
            let v: u32 = range_part.parse().map_err(|_| "Cron 数值无效。")?;
            (v, v)
        };
        let upper = if allow_sunday_seven { max.max(7) } else { max };
        if start < min || end > upper {
            return Err(format!("Cron 字段值超出范围（{min}–{max}）。"));
        }
        let mut cur = start;
        while cur <= end {
            let normalized = if allow_sunday_seven && cur == 0 {
                7
            } else {
                cur
            };
            set.insert(normalized);
            cur += step;
        }
    }
    if set.is_empty() {
        return Err("Cron 字段未匹配任何值。".into());
    }
    Ok(set)
}

pub fn compute_next_run(
    schedule_kind: &str,
    interval_minutes: Option<i64>,
    time_of_day: Option<&str>,
    weekdays: Option<&str>,
    cron_expr: Option<&str>,
    after_ts: i64,
) -> Result<i64, String> {
    match schedule_kind {
        "interval" => {
            let mins = interval_minutes.ok_or("请设置执行间隔（分钟）。")?;
            if !(1..=43_200).contains(&mins) {
                return Err("执行间隔需在 1 分钟到 30 天之间。".into());
            }
            Ok(after_ts + mins * 60)
        }
        "daily" => {
            let (hour, minute) = parse_time_of_day(time_of_day.ok_or("请设置每日执行时间。")?)?;
            let base = epoch_to_local(after_ts).ok_or("无法获取本地时间。")?;
            for offset in 0..=2 {
                let probe = epoch_to_local(after_ts + offset * 86_400).ok_or("无法计算日期。")?;
                if let Some(candidate) = local_to_epoch(probe.year, probe.month, probe.day, hour, minute) {
                    if candidate > after_ts {
                        return Ok(candidate);
                    }
                }
            }
            local_to_epoch(base.year, base.month, base.day, hour, minute)
                .map(|t| t + 86_400)
                .ok_or_else(|| "无法计算下次执行时间。".into())
        }
        "weekly" => {
            let (hour, minute) = parse_time_of_day(time_of_day.ok_or("请设置每周执行时间。")?)?;
            let days = parse_weekdays(weekdays.ok_or("请选择每周执行的星期。")?)?;
            for offset in 0..=8 {
                let probe = epoch_to_local(after_ts + offset * 86_400).ok_or("无法计算日期。")?;
                if !days.contains(&probe.weekday) {
                    continue;
                }
                if let Some(candidate) = local_to_epoch(probe.year, probe.month, probe.day, hour, minute) {
                    if candidate > after_ts {
                        return Ok(candidate);
                    }
                }
            }
            Err("无法计算每周任务的下次执行时间。".into())
        }
        "cron" => {
            let expr = cron_expr.ok_or("请填写 5 段 Cron 表达式。")?.trim();
            let fields: Vec<&str> = expr.split_whitespace().collect();
            if fields.len() != 5 {
                return Err("Cron 表达式需要 5 个字段（分 时 日 月 周），例如 0 9 * * 1-5。".into());
            }
            let minutes = parse_cron_field(fields[0], 0, 59, false)?;
            let hours = parse_cron_field(fields[1], 0, 23, false)?;
            let doms = parse_cron_field(fields[2], 1, 31, false)?;
            let months = parse_cron_field(fields[3], 1, 12, false)?;
            let dows = parse_cron_field(fields[4], 0, 6, true)?;
            let dom_any = fields[2].trim() == "*";
            let dow_any = fields[4].trim() == "*";

            // Start from the next whole minute strictly after after_ts
            let mut cursor = ((after_ts / 60) + 1) * 60;
            let limit = after_ts + 366 * 86_400;
            while cursor <= limit {
                let dt = epoch_to_local(cursor).ok_or("无法解析本地时间。")?;
                if !months.contains(&dt.month) {
                    cursor += 86_400;
                    continue;
                }
                let day_match = match (dom_any, dow_any) {
                    (true, true) => true,
                    (false, true) => doms.contains(&dt.day),
                    (true, false) => dows.contains(&dt.weekday),
                    (false, false) => doms.contains(&dt.day) || dows.contains(&dt.weekday),
                };
                if !day_match {
                    // Jump to 00:00 of next day
                    let rem_mins = (24 - dt.hour) * 60 - dt.minute;
                    cursor += (rem_mins.max(1) as i64) * 60;
                    continue;
                }
                if !hours.contains(&dt.hour) {
                    let rem_mins = 60 - dt.minute;
                    cursor += (rem_mins.max(1) as i64) * 60;
                    continue;
                }
                if minutes.contains(&dt.minute) {
                    return Ok(cursor);
                }
                cursor += 60;
            }
            Err("该 Cron 表达式在未来一年内没有匹配的执行时间。".into())
        }
        _ => Err("不支持的定时类型。".into()),
    }
}

/// Convert Codex iCalendar RRULE string into AgentCrew schedule fields.
pub fn parse_codex_rrule(rrule: &str) -> (String, Option<i64>, Option<String>, Option<String>, Option<String>) {
    let mut map = BTreeMap::new();
    for part in rrule.trim().trim_start_matches("RRULE:").split(';') {
        if let Some((k, v)) = part.split_once('=') {
            map.insert(k.trim().to_ascii_uppercase(), v.trim().to_ascii_uppercase());
        }
    }
    let freq = map.get("FREQ").map(String::as_str).unwrap_or("HOURLY");
    let interval: i64 = map
        .get("INTERVAL")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(1);
    let by_hour: Option<u32> = map
        .get("BYHOUR")
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| v <= 23);
    let by_minute: u32 = map
        .get("BYMINUTE")
        .and_then(|v| v.split(',').next())
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| v <= 59)
        .unwrap_or(0);

    match freq {
        "MINUTELY" => (
            "interval".into(),
            Some(interval.clamp(1, 43_200)),
            None,
            None,
            None,
        ),
        "HOURLY" => {
            if interval == 24 {
                if let Some(hour) = by_hour {
                    return (
                        "daily".into(),
                        None,
                        Some(format!("{hour:02}:{by_minute:02}")),
                        None,
                        None,
                    );
                }
                return ("interval".into(), Some(1440), None, None, None);
            }
            (
                "interval".into(),
                Some((interval * 60).clamp(1, 43_200)),
                None,
                None,
                None,
            )
        }
        "DAILY" => {
            let hour = by_hour.unwrap_or(9);
            (
                "daily".into(),
                None,
                Some(format!("{hour:02}:{by_minute:02}")),
                None,
                None,
            )
        }
        "WEEKLY" => {
            let hour = by_hour.unwrap_or(9);
            let mut days = Vec::new();
            if let Some(raw_days) = map.get("BYDAY") {
                for token in raw_days.split(',') {
                    let mapped = match token.trim() {
                        "MO" => Some(1),
                        "TU" => Some(2),
                        "WE" => Some(3),
                        "TH" => Some(4),
                        "FR" => Some(5),
                        "SA" => Some(6),
                        "SU" => Some(7),
                        _ => None,
                    };
                    if let Some(d) = mapped {
                        if !days.contains(&d) {
                            days.push(d);
                        }
                    }
                }
            }
            if days.is_empty() {
                days.push(1);
            }
            days.sort_unstable();
            let weekdays = days
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(",");
            (
                "weekly".into(),
                None,
                Some(format!("{hour:02}:{by_minute:02}")),
                Some(weekdays),
                None,
            )
        }
        _ => ("daily".into(), None, Some("09:00".into()), None, None),
    }
}
