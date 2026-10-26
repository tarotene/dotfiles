//! Bounded random assignment of the execute reasoning effort. The assignment is
//! per fresh launch; the assigned value is never recorded here because native
//! history already records each turn's effort. Only the period is kept, so that
//! `compare` can tell assigned threads from threads whose setting was chosen.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Experiment {
    pub execute_efforts: Vec<String>,
    /// Local calendar day (YYYY-MM-DD) on which `experiment start` ran.
    pub start: String,
    /// Last local calendar day on which a fresh launch still draws.
    pub until: String,
    /// Set by `experiment stop`; kept so the assigned period stays derivable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        _ => 28,
    }
}

/// Strict `YYYY-MM-DD`. ISO dates compare correctly as strings only in this shape.
pub fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| s[r].bytes().all(|c| c.is_ascii_digit());
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let (year, month, day): (u32, u32, u32) = (
        s[0..4].parse().unwrap(),
        s[5..7].parse().unwrap(),
        s[8..10].parse().unwrap(),
    );
    (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

impl Experiment {
    pub fn start(efforts: Vec<String>, today: &str, until: &str) -> Result<Self> {
        let experiment = Self {
            execute_efforts: efforts,
            start: today.into(),
            until: until.into(),
            ended: None,
        };
        experiment.validate()?;
        Ok(experiment)
    }

    /// A running experiment can only be extended: same candidates, later last day.
    /// `start` and the assigned period stay, so earlier launches keep their label.
    pub fn extend(&self, efforts: &[String], until: &str) -> Result<Self> {
        if self.execute_efforts != efforts {
            bail!("an experiment with different efforts is already running; run `codex-plan experiment stop` first");
        }
        let mut next = self.clone();
        next.until = until.into();
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<()> {
        if self.execute_efforts.len() < 2 {
            bail!("experiment needs at least two distinct execute efforts to compare");
        }
        for (i, effort) in self.execute_efforts.iter().enumerate() {
            if effort.is_empty() || self.execute_efforts[..i].contains(effort) {
                bail!("experiment execute_efforts must be non-empty and distinct");
            }
        }
        for (name, day) in [
            ("start", Some(&self.start)),
            ("until", Some(&self.until)),
            ("ended", self.ended.as_ref()),
        ] {
            if let Some(day) = day {
                if !valid_date(day) {
                    bail!("experiment {name} must be a YYYY-MM-DD date: {day:?}");
                }
            }
        }
        if self.until < self.start {
            bail!(
                "experiment until ({}) is before start ({})",
                self.until,
                self.start
            );
        }
        if self.ended.as_ref().is_some_and(|e| *e < self.start) {
            bail!("experiment ended is before start");
        }
        Ok(())
    }

    /// Draws only before `until` and only until `experiment stop`.
    pub fn is_active(&self, today: &str) -> bool {
        self.ended.is_none() && today <= self.until.as_str()
    }

    /// Last day whose fresh launches count as assigned.
    pub fn window_end(&self) -> &str {
        self.ended.as_deref().unwrap_or(&self.until)
    }

    pub fn covers_day(&self, day: &str) -> bool {
        self.start.as_str() <= day && day <= self.window_end()
    }

    /// Uniform choice by rejection sampling; `random` yields independent bytes.
    pub fn pick(&self, random: &mut dyn FnMut() -> Result<u8>) -> Result<&str> {
        let n = self.execute_efforts.len();
        let limit = 256 - 256 % n;
        loop {
            let byte = usize::from(random()?);
            if byte < limit {
                return Ok(&self.execute_efforts[byte % n]);
            }
        }
    }

    pub fn status_line(&self, today: &str) -> String {
        let efforts = self.execute_efforts.join("/");
        match &self.ended {
            Some(ended) => format!(
                "experiment: stopped {ended} (assigned {} .. {ended}; efforts {efforts})",
                self.start
            ),
            None if today <= self.until.as_str() => format!(
                "experiment: running, fresh launches draw {efforts} through {} (since {})",
                self.until, self.start
            ),
            None => format!(
                "experiment: expired {} (since {}; efforts {efforts}); run `codex-plan experiment stop`",
                self.until, self.start
            ),
        }
    }
}

pub fn urandom_byte() -> Result<u8> {
    let mut byte = [0u8; 1];
    std::fs::File::open("/dev/urandom")
        .context("open /dev/urandom")?
        .read_exact(&mut byte)
        .context("read /dev/urandom")?;
    Ok(byte[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> Vec<String> {
        vec!["low".into(), "medium".into()]
    }

    #[test]
    fn dates_are_strict_iso_days() {
        for ok in ["2026-10-27", "2028-02-29", "2000-02-29"] {
            assert!(valid_date(ok), "{ok}");
        }
        for bad in [
            "2026-1-27",
            "2026-13-01",
            "2026-02-29",
            "1900-02-29",
            "2026-10-32",
            "2026/10/27",
            "26-10-27",
            "2026-10-2a",
            "",
        ] {
            assert!(!valid_date(bad), "{bad}");
        }
    }

    #[test]
    fn needs_two_distinct_efforts_and_ordered_dates() {
        assert!(Experiment::start(pair(), "2026-10-27", "2026-11-01").is_ok());
        assert!(Experiment::start(vec!["low".into()], "2026-10-27", "2026-11-01").is_err());
        assert!(
            Experiment::start(vec!["low".into(), "low".into()], "2026-10-27", "2026-11-01")
                .is_err()
        );
        assert!(Experiment::start(pair(), "2026-10-27", "2026-10-26").is_err());
        assert!(Experiment::start(pair(), "2026-10-27", "soon").is_err());
        assert!(
            Experiment::start(pair(), "2026-10-27", "2026-10-27").is_ok(),
            "same-day experiment"
        );
    }

    #[test]
    fn a_running_experiment_extends_only_with_the_same_candidates() {
        let e = Experiment::start(pair(), "2026-10-27", "2026-10-30").unwrap();
        let longer = e.extend(&pair(), "2026-11-03").unwrap();
        assert_eq!(
            (longer.start.as_str(), longer.until.as_str()),
            ("2026-10-27", "2026-11-03")
        );
        assert!(e
            .extend(&["low".into(), "high".into()], "2026-11-03")
            .is_err());
        assert!(
            e.extend(&pair(), "2026-10-26").is_err(),
            "last day cannot precede start"
        );
    }

    #[test]
    fn drawing_stops_after_until_or_stop_and_window_follows_ended() {
        let mut e = Experiment::start(pair(), "2026-10-27", "2026-11-01").unwrap();
        assert!(e.is_active("2026-10-27") && e.is_active("2026-11-01"));
        assert!(!e.is_active("2026-11-02"));
        assert_eq!(e.window_end(), "2026-11-01");
        e.ended = Some("2026-10-29".into());
        assert!(!e.is_active("2026-10-28"), "stop wins over until");
        assert!(
            e.covers_day("2026-10-29")
                && !e.covers_day("2026-10-30")
                && !e.covers_day("2026-10-26")
        );
        e.validate().unwrap();
        e.ended = Some("2026-10-26".into());
        assert!(e.validate().is_err());
    }

    #[test]
    fn pick_is_uniform_over_all_bytes_and_rejects_the_biased_tail() {
        let e = Experiment::start(
            vec!["a".into(), "b".into(), "c".into()],
            "2026-10-27",
            "2026-10-28",
        )
        .unwrap();
        let mut counts = [0usize; 3];
        for b in 0..=255u8 {
            let mut once = Some(b);
            // Bytes 255 (256 % 3 == 1) is rejected; the sampler asks again.
            let mut source = || -> Result<u8> { Ok(once.take().unwrap_or(0)) };
            let chosen = e.pick(&mut source).unwrap();
            counts[e.execute_efforts.iter().position(|x| x == chosen).unwrap()] += 1;
        }
        // 255 accepted bytes split 85/85/85, plus the rejected one retried as 0 -> "a".
        assert_eq!(counts, [86, 85, 85]);
    }

    #[test]
    fn toml_round_trip_omits_ended_until_stopped() {
        let e = Experiment::start(pair(), "2026-10-27", "2026-11-01").unwrap();
        let text = toml::to_string_pretty(&e).unwrap();
        assert!(!text.contains("ended"));
        assert_eq!(toml::from_str::<Experiment>(&text).unwrap(), e);
        assert!(toml::from_str::<Experiment>(
            "execute_efforts=['a','b']\nstart='2026-10-27'\nuntil='2026-10-28'\nextra=1"
        )
        .is_err());
    }
}
