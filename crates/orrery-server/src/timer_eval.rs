use chrono::{DateTime, Utc};
use orrery::model::{TimerDefinition, TimerKind};

/// Compute the absolute `due_at` timestamp for a timer definition.
/// For Duration: now + duration. For Date: the parsed timestamp. For Cycle: next occurrence.
pub fn evaluate_due_at(definition: &TimerDefinition) -> Result<DateTime<Utc>, String> {
    match definition.kind {
        TimerKind::Duration => {
            let dur = parse_iso_duration(&definition.expression);
            Ok(Utc::now() + dur)
        }
        TimerKind::Date => definition
            .expression
            .parse::<DateTime<Utc>>()
            .map_err(|e| format!("invalid timeDate '{}': {}", definition.expression, e)),
        TimerKind::Cycle => next_cycle_due_at(&definition.expression),
    }
}

/// Parse an ISO 8601 duration string into a chrono::Duration.
/// Supports: PxD, PTxH, PTxM, PTxS and combinations. No months/years.
pub fn parse_iso_duration(s: &str) -> chrono::Duration {
    let s = s.trim().trim_start_matches('P');
    let (date_part, time_part) = if let Some(t_pos) = s.find('T') {
        (&s[..t_pos], &s[t_pos + 1..])
    } else {
        (s, "")
    };

    let mut total_secs: i64 = 0;

    if let Some(d) = date_part
        .strip_suffix('D')
        .and_then(|v| v.parse::<i64>().ok())
    {
        total_secs += d * 86400;
    }

    let mut remaining = time_part;
    for (suffix, multiplier) in [("H", 3600_i64), ("M", 60), ("S", 1)] {
        if let Some(pos) = remaining.find(suffix) {
            if let Ok(v) = remaining[..pos].parse::<i64>() {
                total_secs += v * multiplier;
            }
            remaining = &remaining[pos + 1..];
        }
    }

    chrono::Duration::seconds(total_secs)
}

/// Extract the period/interval from an ISO 8601 repeating interval.
/// "R3/PT10H" → "PT10H", "R/P1D" → "P1D", bare "PT5M" → "PT5M"
pub(crate) fn extract_cycle_interval(expr: &str) -> String {
    if let Some(slash) = expr.find('/') {
        expr[slash + 1..].to_string()
    } else {
        expr.to_string()
    }
}

/// Decrement the repetition count in an ISO 8601 repeating interval expression.
///
/// Returns `Some(new_expr)` if there are repetitions remaining, `None` if exhausted.
/// - `"R3/PT10H"` → `Some("R2/PT10H")`
/// - `"R1/PT10H"` → `None` (last repetition just fired)
/// - `"R/PT10H"` (infinite) → `Some("R/PT10H")` (unchanged)
/// - Non-R expressions → `None` (treat as single occurrence, no re-schedule)
pub fn decrement_cycle_count(expr: &str) -> Option<String> {
    if !expr.starts_with('R') {
        return None; // Plain duration or date — not a repeating cycle
    }
    let slash = expr.find('/')?;
    let repeat_part = &expr[1..slash]; // "" for infinite, "3" for R3
    let interval_part = &expr[slash..]; // "/PT10H"

    if repeat_part.is_empty() {
        return Some(expr.to_string()); // Infinite cycle — keep firing
    }
    let count: u32 = repeat_part.parse().ok()?;
    if count <= 1 {
        None // Exhausted
    } else {
        Some(format!("R{}{}", count - 1, interval_part))
    }
}

/// Compute the next trigger time for an ISO 8601 repeating interval or cron expression.
///
/// - ISO repeating: `"R3/PT10H"`, `"R/P1D"` — fires after the interval duration.
/// - Cron: `"0 * * * * *"` — standard 6-field cron (second minute hour day month weekday).
///   Detected by the presence of spaces (ISO 8601 expressions never contain spaces).
pub fn next_cycle_due_at(expr: &str) -> Result<DateTime<Utc>, String> {
    if expr.contains(' ') {
        use cron::Schedule;
        use std::str::FromStr;
        let schedule =
            Schedule::from_str(expr).map_err(|e| format!("invalid cron '{}': {}", expr, e))?;
        schedule
            .upcoming(Utc)
            .next()
            .ok_or_else(|| format!("cron '{}' has no future occurrences", expr))
    } else {
        // ISO 8601 repeating interval: R[n]/P[duration] or just P[duration]
        let interval = extract_cycle_interval(expr);
        let dur = parse_iso_duration(&interval);
        if dur.is_zero() {
            return Err(format!(
                "cycle interval '{}' parses to zero duration",
                interval
            ));
        }
        Ok(Utc::now() + dur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use orrery::model::{TimerDefinition, TimerKind};

    #[test]
    fn evaluate_duration_adds_to_now() {
        let def = TimerDefinition {
            kind: TimerKind::Duration,
            expression: "PT5M".to_string(),
        };
        let due = evaluate_due_at(&def).unwrap();
        let now = Utc::now();
        let diff = due - now;
        assert!(
            diff.num_seconds() > 290 && diff.num_seconds() < 310,
            "Expected ~5 min from now, got {}",
            diff.num_seconds()
        );
    }

    #[test]
    fn evaluate_date_parses_iso_timestamp() {
        let def = TimerDefinition {
            kind: TimerKind::Date,
            expression: "2030-01-01T00:00:00Z".to_string(),
        };
        let due = evaluate_due_at(&def).unwrap();
        assert_eq!(due.year(), 2030);
    }

    #[test]
    fn evaluate_cycle_uses_interval_duration() {
        let def = TimerDefinition {
            kind: TimerKind::Cycle,
            expression: "R3/PT10M".to_string(),
        };
        let due = evaluate_due_at(&def).unwrap();
        let now = Utc::now();
        let diff = due - now;
        assert!(
            diff.num_seconds() > 590 && diff.num_seconds() < 610,
            "Expected ~10 min from now for cycle, got {}",
            diff.num_seconds()
        );
    }

    #[test]
    fn parse_iso_duration_five_minutes() {
        let d = parse_iso_duration("PT5M");
        assert_eq!(d.num_seconds(), 300);
    }

    #[test]
    fn parse_iso_duration_one_day() {
        let d = parse_iso_duration("P1D");
        assert_eq!(d.num_seconds(), 86400);
    }

    #[test]
    fn parse_iso_duration_combined() {
        let d = parse_iso_duration("P1DT2H30M");
        assert_eq!(d.num_seconds(), 86400 + 7200 + 1800);
    }

    #[test]
    fn extract_cycle_interval_strips_repeat_prefix() {
        assert_eq!(extract_cycle_interval("R3/PT10H"), "PT10H");
        assert_eq!(extract_cycle_interval("R/P1D"), "P1D");
        assert_eq!(extract_cycle_interval("PT5M"), "PT5M");
    }

    #[test]
    fn decrement_cycle_count_decrements() {
        assert_eq!(
            decrement_cycle_count("R3/PT10H"),
            Some("R2/PT10H".to_string())
        );
        assert_eq!(decrement_cycle_count("R2/P1D"), Some("R1/P1D".to_string()));
    }

    #[test]
    fn decrement_cycle_count_exhausted() {
        assert_eq!(decrement_cycle_count("R1/PT10H"), None);
    }

    #[test]
    fn decrement_cycle_count_infinite() {
        assert_eq!(
            decrement_cycle_count("R/PT10H"),
            Some("R/PT10H".to_string())
        );
    }

    #[test]
    fn decrement_cycle_count_non_repeating() {
        assert_eq!(decrement_cycle_count("PT10H"), None);
    }

    #[test]
    fn next_cycle_due_at_repeating_duration() {
        let due = next_cycle_due_at("R3/PT10H").unwrap();
        let diff = due - Utc::now();
        // ~10 hours = 36000s, allow ±10s for test execution
        assert!(
            diff.num_seconds() > 35990 && diff.num_seconds() < 36010,
            "Expected ~10h from now, got {}s",
            diff.num_seconds()
        );
    }

    #[test]
    fn next_cycle_due_at_unlimited_repeating() {
        let due = next_cycle_due_at("R/P1D").unwrap();
        let diff = due - Utc::now();
        // ~1 day = 86400s
        assert!(
            diff.num_seconds() > 86390 && diff.num_seconds() < 86410,
            "Expected ~1 day from now, got {}s",
            diff.num_seconds()
        );
    }

    #[test]
    fn next_cycle_due_at_cron() {
        // Every minute (6-field cron: sec min hour day month weekday)
        let due = next_cycle_due_at("0 * * * * *").unwrap();
        let diff = due - Utc::now();
        assert!(
            diff.num_seconds() >= 0 && diff.num_seconds() <= 60,
            "Expected cron next occurrence within 60s, got {}s",
            diff.num_seconds()
        );
    }

    #[test]
    fn next_cycle_due_at_invalid_cron() {
        let result = next_cycle_due_at("not a cron expression");
        // Contains no spaces so treated as ISO; parses to zero → error
        assert!(result.is_err());
    }
}
