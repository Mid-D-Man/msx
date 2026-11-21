// core/msx-svg-import/src/syncbase.rs
//! `begin` values that name another animation (`a.begin+0.2s`, `a.end`,
//! `0;z.end+0.25s`), resolved to times.
//!
//! NOTICE: see docs/guide/svg-import.md, section "Animation". The scene has no
//! input, so a start that waits for another animation can only be a number the
//! document already fixes. This module reads the list, takes every animation of
//! the document together and finds the times at which each one starts, as SMIL
//! does: a start of `a.end` is every moment `a` ends, and a start of `a.begin`
//! every moment `a` starts. Animations that start each other in a ring (a
//! spinner whose last step begins the first again) have a start list that never
//! ends, so the search stops after `MAX_INSTANCES` starts, and a list that keeps
//! its spacing to that point is a repeat with a period.

use crate::anim::parse_clock;

/// Starts kept for one animation. A list that fills this is taken to go on.
pub(crate) const MAX_INSTANCES: usize = 48;
/// No start after this many seconds is looked for.
pub(crate) const HORIZON: f64 = 600.0;
/// Passes of the search. Each pass moves every start list one step along a
/// chain, so a ring of `n` animations needs about `n` passes for each start of
/// the first. This leaves room for a ring of 16 that fills the cap.
const MAX_PASSES: usize = MAX_INSTANCES * 16 + 32;
/// Animations the search takes on. A document with more keeps its old behavior.
pub(crate) const MAX_DECLS: usize = 600;

/// One entry of a `begin` list.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BeginItem {
    /// A time in seconds.
    At(f64),
    /// The start (`end = false`) or the end (`end = true`) of the animation
    /// with this id, plus an offset.
    Sync { id: String, end: bool, offset: f64 },
    /// `indefinite`: a start that only an event could cause, so it adds none.
    Never,
}

fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

/// One entry. An event, `repeat()` or anything else that needs input is an error.
pub(crate) fn parse_item(item: &str) -> Result<BeginItem, String> {
    let item = item.trim();
    if item == "indefinite" {
        return Ok(BeginItem::Never);
    }
    if let Some(t) = parse_clock(item) {
        return Ok(BeginItem::At(t));
    }
    let waits = || format!("begin=\"{item}\" waits for an event");
    let Some((id, rest)) = item.split_once('.') else { return Err(waits()) };
    if id.is_empty() || !id.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ':')) {
        return Err(waits());
    }
    let (end, tail) = if let Some(t) = rest.strip_prefix("begin") {
        (false, t)
    } else if let Some(t) = rest.strip_prefix("end") {
        (true, t)
    } else {
        return Err(waits());
    };
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(BeginItem::Sync { id: id.to_string(), end, offset: 0.0 });
    }
    let (sign, clock) = match tail.chars().next() {
        Some('+') => (1.0, tail[1..].trim()),
        Some('-') => (-1.0, tail[1..].trim()),
        _ => return Err(waits()),
    };
    match parse_clock(clock) {
        Some(t) => Ok(BeginItem::Sync { id: id.to_string(), end, offset: sign * t }),
        None => Err(format!("begin=\"{item}\" has an offset that is not a time")),
    }
}

/// A whole `begin` value.
pub(crate) fn parse_list(raw: &str) -> Result<Vec<BeginItem>, String> {
    raw.split(';').map(str::trim).filter(|i| !i.is_empty()).map(parse_item).collect()
}

/// Whether a `begin` value names another animation. Only then does this
/// module decide the start; a list of plain times keeps the earlier rule.
pub(crate) fn has_syncbase(raw: &str) -> bool {
    parse_list(raw).is_ok_and(|items| items.iter().any(|i| matches!(i, BeginItem::Sync { .. })))
        || raw.split(';').any(|i| {
            let i = i.trim();
            parse_clock(i).is_none() && i != "indefinite" && i.split_once('.').is_some_and(|(id, rest)| !id.is_empty() && (rest.starts_with("begin") || rest.starts_with("end")))
        })
}

/// What the search needs to know about one animation.
#[derive(Debug, Clone)]
pub(crate) struct Decl {
    pub id: Option<String>,
    /// The `begin` list, or why it cannot be read.
    pub begin: Result<Vec<BeginItem>, String>,
    /// How long one start keeps it active, or `None` when it never ends.
    pub active: Option<f64>,
}

/// The moments an animation ends, given the moments it starts. A start while
/// it is active restarts it, which ends the run before it at that moment.
fn ends_of(begins: &[f64], active: Option<f64>) -> Vec<f64> {
    let mut out = Vec::new();
    for (k, b) in begins.iter().enumerate() {
        let natural = active.map(|a| b + a);
        let next = begins.get(k + 1).copied();
        match (natural, next) {
            (Some(n), Some(x)) => out.push(round6(n.min(x))),
            (Some(n), None) => out.push(round6(n)),
            (None, Some(x)) => out.push(round6(x)),
            (None, None) => {}
        }
    }
    out
}

/// The start times of every animation, in the order of `decls`. An animation
/// whose list cannot be read, or that names an id nobody has, gets none.
pub(crate) fn solve(decls: &[Decl]) -> Vec<Vec<f64>> {
    let by_id: std::collections::HashMap<&str, usize> = decls.iter().enumerate().filter_map(|(i, d)| d.id.as_deref().map(|id| (id, i))).collect();
    let mut inst: Vec<Vec<f64>> = vec![Vec::new(); decls.len()];
    for _ in 0..MAX_PASSES {
        let ends: Vec<Vec<f64>> = decls.iter().zip(&inst).map(|(d, b)| ends_of(b, d.active)).collect();
        let mut changed = false;
        for (i, decl) in decls.iter().enumerate() {
            let Ok(items) = &decl.begin else { continue };
            let mut list: Vec<f64> = Vec::new();
            for item in items {
                match item {
                    BeginItem::At(t) => list.push(round6(*t)),
                    BeginItem::Never => {}
                    BeginItem::Sync { id, end, offset } => {
                        if let Some(&j) = by_id.get(id.as_str()) {
                            let source = if *end { &ends[j] } else { &inst[j] };
                            list.extend(source.iter().map(|t| round6(t + offset)));
                        }
                    }
                }
            }
            list.retain(|t| *t <= HORIZON);
            list.sort_by(|a, b| a.total_cmp(b));
            list.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
            list.truncate(MAX_INSTANCES);
            if list != inst[i] {
                inst[i] = list;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    inst
}

/// What a list of start times comes to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Resolution {
    /// One start.
    Once(f64),
    /// Starts at `first`, then every `period` seconds, for as long as the scene runs.
    Periodic { first: f64, period: f64 },
}

/// Reads a list of start times as one start or an even repeat.
pub(crate) fn classify(list: &[f64]) -> Result<Resolution, String> {
    match list {
        [] => Err("its begin depends on an animation that never starts".into()),
        [t] => Ok(Resolution::Once(*t)),
        _ => {
            let period = list[1] - list[0];
            let even = list.windows(2).all(|w| ((w[1] - w[0]) - period).abs() < 1e-5);
            let goes_on = list.len() >= MAX_INSTANCES || list[list.len() - 1] + period > HORIZON;
            if even && period > 1e-6 && list.len() >= 3 && goes_on {
                Ok(Resolution::Periodic { first: round6(list[0]), period: round6(period) })
            } else {
                Err("it starts several times, but not at an even spacing for as long as the scene runs".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn decl(id: &str, begin: &str, active: Option<f64>) -> Decl {
        Decl { id: Some(id.to_string()), begin: parse_list(begin), active }
    }

    #[test]
    fn items_read_times_syncbase_offsets_and_refuse_events() {
        assert_eq!(parse_item("0s"), Ok(BeginItem::At(0.0)));
        assert_eq!(parse_item("-1.5s"), Ok(BeginItem::At(-1.5)));
        assert_eq!(parse_item("indefinite"), Ok(BeginItem::Never));
        assert_eq!(parse_item("a.begin"), Ok(BeginItem::Sync { id: "a".into(), end: false, offset: 0.0 }));
        assert_eq!(parse_item("a.end"), Ok(BeginItem::Sync { id: "a".into(), end: true, offset: 0.0 }));
        assert_eq!(parse_item("spinner_oJFS.begin+0.2s"), Ok(BeginItem::Sync { id: "spinner_oJFS".into(), end: false, offset: 0.2 }));
        assert_eq!(parse_item("x-1.end-0.25s"), Ok(BeginItem::Sync { id: "x-1".into(), end: true, offset: -0.25 }));
        assert_eq!(parse_item("a.end + 500ms"), Ok(BeginItem::Sync { id: "a".into(), end: true, offset: 0.5 }));
        for bad in ["click", "a.click", "a.repeat(2)", "a.endEvent", "a.end+x", "a.end 1s", ".end", "wallclock(1)", "a b.end"] {
            assert!(parse_item(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_list_is_parsed_whole_and_syncbase_is_told_apart() {
        assert_eq!(parse_list("0;z.end+0.25s").unwrap().len(), 2);
        assert!(parse_list("0;click").is_err());
        assert!(has_syncbase("0;z.end+0.25s"));
        assert!(has_syncbase("a.begin+1s"));
        assert!(!has_syncbase("0s"));
        assert!(!has_syncbase("0;1s;2.5s"));
        assert!(!has_syncbase("click"));
        // A list with an event next to a syncbase item still counts as syncbase, so it is reported, not guessed.
        assert!(has_syncbase("a.end;click"));
    }

    #[test]
    fn a_chain_of_offsets_resolves_in_one_pass_each() {
        let d = vec![decl("a", "1s", Some(2.0)), decl("b", "a.begin+0.5s", Some(2.0)), decl("c", "a.end", Some(1.0)), decl("d", "c.end-0.25s", Some(1.0))];
        let s = solve(&d);
        assert_eq!(s[0], vec![1.0]);
        assert_eq!(s[1], vec![1.5]);
        assert_eq!(s[2], vec![3.0]);
        assert_eq!(s[3], vec![3.75]);
    }

    #[test]
    fn a_ring_of_animations_starts_over_at_an_even_period() {
        // The shape of the blocks in a spinner: a runs 0 to 0.2, b follows a, c follows b, and a starts over after c.
        let d = vec![decl("a", "0;c.end", Some(0.2)), decl("b", "a.end", Some(0.2)), decl("c", "b.end", Some(0.2))];
        let s = solve(&d);
        assert!(s[0].len() >= MAX_INSTANCES - 1, "a keeps starting: {}", s[0].len());
        assert!(close(s[0][1] - s[0][0], 0.6) && close(s[1][0], 0.2) && close(s[2][0], 0.4));
        assert_eq!(classify(&s[0]), Ok(Resolution::Periodic { first: 0.0, period: 0.6 }));
        assert_eq!(classify(&s[2]), Ok(Resolution::Periodic { first: 0.4, period: 0.6 }));
    }

    #[test]
    fn a_ring_with_a_gap_after_the_last_step_keeps_the_gap_in_the_period() {
        // Three dots 0.2 s apart, each 0.6 s long, and the first one starts again 0.25 s after the last ends.
        let d = vec![decl("a", "0;c.end+0.25s", Some(0.6)), decl("b", "a.begin+0.2s", Some(0.6)), decl("c", "b.begin+0.2s", Some(0.6))];
        let s = solve(&d);
        // c starts at 0.4 and ends at 1.0, so a starts again at 1.25: a period of 1.25.
        assert_eq!(classify(&s[0]), Ok(Resolution::Periodic { first: 0.0, period: 1.25 }));
        assert_eq!(classify(&s[1]), Ok(Resolution::Periodic { first: 0.2, period: 1.25 }));
        assert_eq!(classify(&s[2]), Ok(Resolution::Periodic { first: 0.4, period: 1.25 }));
    }

    #[test]
    fn two_animations_that_start_each_other_share_a_period() {
        // The rings of a pulse: each starts 0.6 s after the other, and both last 1.2 s.
        let d = vec![decl("a", "0;b.begin+0.6s", Some(1.2)), decl("b", "a.begin+0.6s", Some(1.2))];
        let s = solve(&d);
        assert_eq!(classify(&s[0]), Ok(Resolution::Periodic { first: 0.0, period: 1.2 }));
        assert_eq!(classify(&s[1]), Ok(Resolution::Periodic { first: 0.6, period: 1.2 }));
    }

    #[test]
    fn a_start_inside_an_active_run_restarts_it_and_ends_the_run_there() {
        // b ends where a restarts it, at 0.5, not at its own 2 s.
        let ends = ends_of(&[0.0, 0.5], Some(2.0));
        assert_eq!(ends, vec![0.5, 2.5]);
        assert_eq!(ends_of(&[0.0], None), Vec::<f64>::new(), "an animation that never ends has no end");
        assert_eq!(ends_of(&[0.0, 3.0], None), vec![3.0], "but a restart ends it");
    }

    #[test]
    fn what_cannot_be_resolved_is_an_error_and_not_a_guess() {
        assert!(classify(&[]).is_err());
        assert_eq!(classify(&[2.5]), Ok(Resolution::Once(2.5)));
        // Two starts, or uneven ones, or a finite series: no single repeat.
        assert!(classify(&[0.0, 1.0]).is_err());
        assert!(classify(&[0.0, 1.0, 3.0, 4.0]).is_err());
        assert!(classify(&[0.0, 1.0, 2.0]).is_err(), "it stops, so it is not a repeat");
        // An id nobody has, an event, and a ring that never gets its first start.
        let d = vec![decl("a", "ghost.end", Some(1.0)), decl("b", "c.end", Some(1.0)), decl("c", "b.end", Some(1.0))];
        let s = solve(&d);
        assert!(s.iter().all(|l| l.is_empty()));
        assert!(classify(&s[0]).is_err());
        let ev = Decl { id: Some("e".into()), begin: parse_list("click"), active: Some(1.0) };
        assert!(solve(&[ev])[0].is_empty());
    }

    #[test]
    fn the_search_stays_within_its_limits() {
        // A ring with a very short period fills the cap and is cut at it.
        let d = vec![decl("a", "0;a.end", Some(0.001))];
        let s = solve(&d);
        assert_eq!(s[0].len(), MAX_INSTANCES);
        // Nothing after the horizon is kept.
        let far = vec![decl("a", "100s;a.end+500s", Some(1.0))];
        let s = solve(&far);
        assert!(s[0].iter().all(|t| *t <= HORIZON), "{:?}", s[0]);
    }
}
