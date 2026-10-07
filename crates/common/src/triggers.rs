//! Deciding WHEN a sound plays. The game gives no "a swing started" or "a shot was fired" event to listen to, so the hook
//! polls numbers it can read (stamina, ammunition counts, whether an attack button is down) once per frame and these
//! small state machines turn that into events. Pure and deterministic, so they are tested with made-up traces on any OS
//! and tuned from the logs of a real play session.
use std::collections::HashMap;

/// Stamina must fall by at least this much between two frames for it to count as the start of an action. Sprinting and
/// resting move stamina by a fraction of a point per frame, attacks and rolls by a dozen points at once.
pub const MIN_STAMINA_DROP: i32 = 5;
/// A press of an attack button explains a stamina drop for this long afterwards.
pub const ATTACK_WINDOW_MS: u64 = 350;
/// A swing within this long after the previous one is the next step of a combo.
pub const COMBO_WINDOW_MS: u64 = 1400;
/// Two swing sounds are never closer together than this.
pub const DEBOUNCE_MS: u64 = 160;
/// Combos cycle through this many different light-attack sounds.
pub const COMBO_STEPS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swing {
    /// a normal attack; `step` 1..=COMBO_STEPS counts the attacks of the current combo
    Light { step: u8 },
    /// an attack made with the strong-attack button
    Strong,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SwingInput {
    pub now_ms: u64,
    pub stamina: i32,
    pub light_down: bool,
    pub strong_down: bool,
}

/// "An attack started" = stamina fell sharply right after an attack button went down. A fall without a press (a roll,
/// a blocked hit) is not a swing, and a press without a fall (no stamina left) is not either.
#[derive(Default)]
pub struct SwingDetector {
    last_stamina: Option<i32>,
    prev_light: bool,
    prev_strong: bool,
    light_edge: Option<u64>,
    strong_edge: Option<u64>,
    last_swing: Option<u64>,
    step: u8,
}

impl SwingDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, i: SwingInput) -> Option<Swing> {
        if i.light_down && !self.prev_light {
            self.light_edge = Some(i.now_ms);
        }
        if i.strong_down && !self.prev_strong {
            self.strong_edge = Some(i.now_ms);
        }
        self.prev_light = i.light_down;
        self.prev_strong = i.strong_down;

        let drop = self.last_stamina.map_or(0, |l| l - i.stamina);
        self.last_stamina = Some(i.stamina);
        if drop < MIN_STAMINA_DROP {
            return None;
        }
        let fresh = |edge: Option<u64>| edge.filter(|t| i.now_ms.saturating_sub(*t) <= ATTACK_WINDOW_MS);
        let (light, strong) = (fresh(self.light_edge), fresh(self.strong_edge));
        if light.is_none() && strong.is_none() {
            return None;
        }
        if self.last_swing.is_some_and(|t| i.now_ms.saturating_sub(t) < DEBOUNCE_MS) {
            return None;
        }
        // the press that happened last explains the drop
        let is_strong = match (light, strong) {
            (Some(l), Some(s)) => s >= l,
            (None, Some(_)) => true,
            _ => false,
        };
        self.light_edge = None;
        self.strong_edge = None;
        let in_combo = self.last_swing.is_some_and(|t| i.now_ms.saturating_sub(t) <= COMBO_WINDOW_MS);
        self.last_swing = Some(i.now_ms);
        if is_strong {
            return Some(Swing::Strong);
        }
        self.step = if in_combo { self.step % COMBO_STEPS + 1 } else { 1 };
        Some(Swing::Light { step: self.step })
    }
}

/// Counts shots by watching ammunition stacks shrink. Only small drops count (1 to 3 at a time): a stack that vanishes
/// or falls by a lot was dropped, sold or lost to a loading screen, not fired.
#[derive(Default)]
pub struct AmmoTracker {
    last: Option<HashMap<u32, u32>>,
}

impl AmmoTracker {
    pub const MAX_SHOTS_AT_ONCE: u32 = 3;

    pub fn new() -> Self {
        Self::default()
    }

    /// `counts` = (item id, quantity) of every ammunition stack in the inventory right now, or `None` when the inventory
    /// could not be read this frame. Returns how many shots were fired since the previous call.
    pub fn update(&mut self, counts: Option<&[(u32, u32)]>) -> u32 {
        let Some(counts) = counts else { return 0 };
        let mut now: HashMap<u32, u32> = HashMap::new();
        for (id, q) in counts {
            *now.entry(*id).or_default() += *q;
        }
        let mut shots = 0;
        if let Some(last) = &self.last {
            for (id, old) in last {
                let new = now.get(id).copied().unwrap_or(0);
                if new < *old && old - new <= Self::MAX_SHOTS_AT_ONCE {
                    shots += old - new;
                }
            }
        }
        self.last = Some(now);
        shots
    }
}

/// Reports the moment a key goes from up to down.
#[derive(Default)]
pub struct Edge(bool);

impl Edge {
    pub fn rising(&mut self, down: bool) -> bool {
        let r = down && !self.0;
        self.0 = down;
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive a detector with (time, stamina, light, strong) rows and collect (time, swing).
    fn run(rows: &[(u64, i32, bool, bool)]) -> Vec<(u64, Swing)> {
        let mut d = SwingDetector::new();
        rows.iter().filter_map(|&(t, s, l, st)| d.update(SwingInput { now_ms: t, stamina: s, light_down: l, strong_down: st }).map(|sw| (t, sw))).collect()
    }

    #[test]
    fn a_press_and_a_stamina_drop_is_a_swing() {
        let out = run(&[(0, 100, false, false), (16, 100, true, false), (33, 82, true, false), (50, 83, false, false)]);
        assert_eq!(out, vec![(33, Swing::Light { step: 1 })]);
    }

    #[test]
    fn a_drop_without_a_press_is_not_a_swing() {
        // a roll: stamina falls 20, no attack button
        assert!(run(&[(0, 100, false, false), (16, 80, false, false)]).is_empty());
        // sprinting: a point or so at a time
        let sprint: Vec<_> = (0..60).map(|k| (k * 16, 100 - k as i32, false, false)).collect();
        assert!(run(&sprint).is_empty());
        // even with the button held down for the whole sprint: small steps never count
        let held: Vec<_> = (0..60).map(|k| (k * 16, 100 - k as i32, true, false)).collect();
        assert!(run(&held).is_empty());
    }

    #[test]
    fn a_press_without_a_drop_is_not_a_swing() {
        assert!(run(&[(0, 0, false, false), (16, 0, true, false), (32, 0, false, false)]).is_empty());
    }

    #[test]
    fn an_old_press_does_not_explain_a_late_drop() {
        let out = run(&[(0, 100, false, false), (10, 100, true, false), (20, 100, false, false), (ATTACK_WINDOW_MS + 100, 80, false, false)]);
        assert!(out.is_empty());
    }

    #[test]
    fn combos_step_through_the_light_sounds_and_start_over_after_a_pause() {
        let mut rows = vec![(0, 100, false, false)];
        let mut t = 0;
        let mut stamina = 100;
        for _ in 0..6 {
            t += 600;
            rows.push((t, stamina, true, false));
            stamina -= 12;
            rows.push((t + 16, stamina, true, false));
            rows.push((t + 40, stamina, false, false));
            stamina += 3; // regenerates a little between swings, never by a drop
        }
        t += 3000; // a long pause ends the combo
        rows.push((t, stamina, true, false));
        rows.push((t + 16, stamina - 12, true, false));
        let steps: Vec<Swing> = run(&rows).into_iter().map(|x| x.1).collect();
        let want: Vec<Swing> = [1u8, 2, 3, 4, 1, 2, 1].iter().map(|&s| Swing::Light { step: s }).collect();
        assert_eq!(steps, want);
    }

    #[test]
    fn the_strong_button_gives_the_strong_sound() {
        let out = run(&[(0, 100, false, false), (16, 100, false, true), (32, 70, false, true)]);
        assert_eq!(out, vec![(32, Swing::Strong)]);
        // the most recent press wins when both were pressed
        let both = run(&[(0, 100, false, false), (10, 100, false, true), (50, 100, true, true), (66, 80, true, true)]);
        assert_eq!(both, vec![(66, Swing::Light { step: 1 })]);
    }

    #[test]
    fn one_press_gives_one_swing_and_a_second_drop_in_a_blink_is_ignored() {
        let out = run(&[(0, 100, false, false), (16, 100, true, false), (32, 85, true, false), (48, 70, true, false)]);
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn the_stamina_jump_of_a_respawn_does_not_trigger_anything() {
        // stamina going UP, or a fresh character starting from nothing, is never a drop
        assert!(run(&[(0, 0, true, false), (16, 100, true, false), (32, 100, true, false)]).is_empty());
    }

    #[test]
    fn shots_are_ammunition_falling_by_one() {
        let mut a = AmmoTracker::new();
        assert_eq!(a.update(Some(&[(404000, 99)])), 0, "the first look only learns the count");
        assert_eq!(a.update(Some(&[(404000, 99)])), 0);
        assert_eq!(a.update(Some(&[(404000, 98)])), 1);
        assert_eq!(a.update(Some(&[(404000, 98)])), 0);
        assert_eq!(a.update(Some(&[(404000, 96)])), 2, "two shots between two frames");
        assert_eq!(a.update(Some(&[(404000, 97)])), 0, "picking ammunition up is not a shot");
        assert_eq!(a.update(Some(&[(404000, 97), (400000, 20)])), 0, "a new stack appearing is not a shot");
        assert_eq!(a.update(Some(&[(404000, 96), (400000, 19)])), 2, "stacks are told apart");
    }

    #[test]
    fn dropping_selling_or_losing_the_inventory_is_not_a_shot() {
        let mut a = AmmoTracker::new();
        a.update(Some(&[(404000, 50)]));
        assert_eq!(a.update(Some(&[(404000, 10)])), 0, "50 -> 10 was a drop/sale");
        assert_eq!(a.update(None), 0, "an unreadable inventory changes nothing");
        assert_eq!(a.update(Some(&[(404000, 9)])), 1, "and the next readable frame still compares with the last good one");
        assert_eq!(a.update(Some(&[])), 0, "a stack of 9 vanishing at once was dropped or sold, not fired");
    }

    #[test]
    fn the_last_bolt_fired_is_still_counted() {
        let mut a = AmmoTracker::new();
        a.update(Some(&[(404000, 1)]));
        assert_eq!(a.update(Some(&[])), 1);
    }

    #[test]
    fn edges_fire_once_per_press() {
        let mut e = Edge::default();
        let seq = [false, true, true, false, true];
        let got: Vec<bool> = seq.iter().map(|&d| e.rising(d)).collect();
        assert_eq!(got, vec![false, true, false, false, true]);
    }
}
