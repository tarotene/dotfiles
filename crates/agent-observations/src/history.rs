use anyhow::{bail, Result};

fn local_tm(timestamp: i64) -> Result<libc::tm> {
    let stamp = timestamp as libc::time_t;
    let mut out = std::mem::MaybeUninit::uninit();
    // SAFETY: both pointers are valid and localtime_r initializes on success.
    if unsafe { libc::localtime_r(&stamp, out.as_mut_ptr()) }.is_null() {
        bail!("timestamp outside local calendar range");
    }
    Ok(unsafe { out.assume_init() })
}

pub fn local_day(timestamp: i64) -> Result<String> {
    let tm = local_tm(timestamp)?;
    Ok(format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    ))
}

pub fn calendar_start(timestamp: i64, offset_days: i64) -> Result<i64> {
    let mut tm = local_tm(timestamp)?;
    tm.tm_hour = 0;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    tm.tm_mday += i32::try_from(offset_days)?;
    tm.tm_isdst = -1;
    // SAFETY: tm is initialized. mktime normalizes day offsets using local DST.
    let result = unsafe { libc::mktime(&mut tm) };
    if result == -1 {
        bail!("calendar day outside supported range");
    }
    Ok(result as i64)
}

pub fn parse_timestamp(value: &str) -> Option<i64> {
    let b = value.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    fn number(b: &[u8]) -> Option<i32> {
        b.iter().try_fold(0, |n, c| {
            c.is_ascii_digit().then(|| n * 10 + i32::from(c - b'0'))
        })
    }
    let year = number(&b[0..4])?;
    let month = number(&b[5..7])?;
    let day = number(&b[8..10])?;
    let hour = number(&b[11..13])?;
    let minute = number(&b[14..16])?;
    let second = number(&b[17..19])?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut pos = 19;
    if b.get(pos) == Some(&b'.') {
        pos += 1;
        let first = pos;
        while b.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if first == pos {
            return None;
        }
    }
    let offset = if b.get(pos) == Some(&b'Z') && pos + 1 == b.len() {
        0
    } else if matches!(b.get(pos), Some(b'+' | b'-')) && b.len() == pos + 6 && b[pos + 3] == b':' {
        let h = number(&b[pos + 1..pos + 3])?;
        let m = number(&b[pos + 4..])?;
        if h > 23 || m > 59 {
            return None;
        }
        (h * 3600 + m * 60) * if b[pos] == b'+' { 1 } else { -1 }
    } else {
        return None;
    };
    // SAFETY: zeroed tm is valid; all relevant calendar fields are set below.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = year - 1900;
    tm.tm_mon = month - 1;
    tm.tm_mday = day;
    tm.tm_hour = hour;
    tm.tm_min = minute;
    tm.tm_sec = second;
    // SAFETY: valid initialized tm pointer. timegm uses UTC, independent of TZ.
    let stamp = unsafe { libc::timegm(&mut tm) };
    if tm.tm_mon != month - 1 || tm.tm_mday != day {
        return None;
    }
    Some(stamp as i64 - i64::from(offset))
}
