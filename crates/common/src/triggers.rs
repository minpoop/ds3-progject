//! Deciding WHEN a sound plays. The game gives no "a swing started" or "a shot was fired" event to listen to, so the hook
//! polls numbers it can read (stamina, ammunition counts, whether an attack button is down, what is in each hand) once
//! per frame and these small state machines turn that into events. Pure and deterministic, so they are tested with
//! made-up traces on any OS and tuned from the logs of a real play session.
use std::collections::{HashMap, VecDeque};

/// Stamina must fall by at least this much between two frames for it to count as the start of an action. Sprinting and
/// resting move stamina by a fraction of a point per frame, attacks and rolls by a dozen points at once.
pub const MIN_STAMINA_DROP: i32 = 5;
/// A press of an attack button explains a stamina drop for this long afterwards.
pub const ATTACK_WINDOW_MS: u64 = 350;
/// A swing within this long after the previous one is the next step of a combo.
pub const COMBO_WINDOW_MS: u64 = 1400;
/// Two attacks are never closer together than this.
pub const DEBOUNCE_MS: u64 = 160;
/// Combos cycle through this many different light-attack sounds.
pub const COMBO_STEPS: u8 = 4;
/// The shots of one burst (the bolt pistol sends three bolts per trigger pull) are played at least this far apart.
pub const SHOT_SPACING_MS: u64 = 110;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hand {
    Right,
    Left,
}

/// What the buttons said an attack was: which hand, and whether it was the strong-attack input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attack {
    pub hand: Hand,
    pub strong: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swing {
    /// a normal attack; `step` 1..=COMBO_STEPS counts the attacks of the current combo
    Light { step: u8 },
    /// an attack made with the strong-attack input
    Strong,
}

/// The attack inputs of one frame. `right_light` / `right_strong` are the right-hand weapon's normal and strong attack
/// (gamepad RB / RT, mouse: left button / Shift + left button), `left_light` the left-hand weapon's attack (gamepad LB,
/// mouse: right button).
#[derive(Clone, Copy, Debug, Default)]
pub struct SwingInput {
    pub now_ms: u64,
    pub stamina: i32,
    pub right_light: bool,
    pub right_strong: bool,
    pub left_light: bool,
}

const INPUTS: [Attack; 3] = [Attack { hand: Hand::Right, strong: false }, Attack { hand: Hand::Right, strong: true }, Attack { hand: Hand::Left, strong: false }];

/// "An attack started" = stamina fell sharply right after an attack input went down. A fall without a press (a roll, a
/// blocked hit) is not an attack, and a press without a fall (no stamina left) is not either. Whether the attack makes a
/// sound is decided by the caller, who knows what is in that hand.
#[derive(Default)]
pub struct SwingDetector {
    last_stamina: Option<i32>,
    prev: [bool; 3],
    edge: [Option<u64>; 3],
    last_attack: Option<u64>,
}

impl SwingDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, i: SwingInput) -> Option<Attack> {
        let down = [i.right_light, i.right_strong, i.left_light];
        for k in 0..3 {
            if down[k] && !self.prev[k] {
                self.edge[k] = Some(i.now_ms);
            }
        }
        self.prev = down;

        let drop = self.last_stamina.map_or(0, |l| l - i.stamina);
        self.last_stamina = Some(i.stamina);
        if drop < MIN_STAMINA_DROP {
            return None;
        }
        // the input pressed last (within the window) explains the drop
        let mut best: Option<(usize, u64)> = None;
        for (k, e) in self.edge.iter().enumerate() {
            if let Some(t) = e.filter(|t| i.now_ms.saturating_sub(*t) <= ATTACK_WINDOW_MS) {
                if best.is_none_or(|(_, bt)| t >= bt) {
                    best = Some((k, t));
                }
            }
        }
        let (k, _) = best?;
        if self.last_attack.is_some_and(|t| i.now_ms.saturating_sub(t) < DEBOUNCE_MS) {
            return None;
        }
        self.edge = [None; 3];
        self.last_attack = Some(i.now_ms);
        Some(INPUTS[k])
    }
}

/// Counts the attacks of a combo: the steps of the light sounds and the window in which the next attack still belongs to it.
#[derive(Default)]
pub struct Combo {
    last: Option<u64>,
    step: u8,
}

impl Combo {
    pub fn new() -> Self {
        Self::default()
    }

    fn in_window(&self, now_ms: u64) -> bool {
        self.last.is_some_and(|t| now_ms.saturating_sub(t) <= COMBO_WINDOW_MS)
    }

    /// The sound for a swing that makes one: light attacks count 1, 2, 3, 4 and around again while they follow each other
    /// within the combo window, and start over from 1 after a pause. A strong attack keeps the window open.
    pub fn swing(&mut self, strong: bool, now_ms: u64) -> Swing {
        let in_combo = self.in_window(now_ms);
        self.last = Some(now_ms);
        if strong {
            return Swing::Strong;
        }
        self.step = if in_combo { self.step % COMBO_STEPS + 1 } else { 1 };
        Swing::Light { step: self.step }
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

/// Lines up the shot sounds of a burst. A bolt that leaves the crossbow is usually seen one frame at a time, but the
/// game may take all three bolts of a burst from the stack in one go: either way the sounds come at least
/// [`SHOT_SPACING_MS`] apart, the first one at once.
#[derive(Default)]
pub struct ShotQueue {
    due: VecDeque<u64>,
    next_free: u64,
}

impl ShotQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// `n` more shots were seen at `now_ms` (at most [`AmmoTracker::MAX_SHOTS_AT_ONCE`] are queued).
    pub fn add(&mut self, n: u32, now_ms: u64) {
        for _ in 0..n.min(AmmoTracker::MAX_SHOTS_AT_ONCE) {
            let at = now_ms.max(self.next_free);
            self.next_free = at + SHOT_SPACING_MS;
            self.due.push_back(at);
        }
    }

    /// How many shot sounds are due by `now_ms` (and no longer waiting).
    pub fn take_due(&mut self, now_ms: u64) -> u32 {
        let mut n = 0;
        while self.due.front().is_some_and(|t| *t <= now_ms) {
            self.due.pop_front();
            n += 1;
        }
        n
    }

    pub fn waiting(&self) -> usize {
        self.due.len()
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

    const RL: (bool, bool, bool) = (true, false, false);
    const RS: (bool, bool, bool) = (false, true, false);
    const LL: (bool, bool, bool) = (false, false, true);
    const NONE: (bool, bool, bool) = (false, false, false);

    /// Drive a detector with (time, stamina, inputs) rows and collect (time, attack).
    fn run(rows: &[(u64, i32, (bool, bool, bool))]) -> Vec<(u64, Attack)> {
        let mut d = SwingDetector::new();
        rows.iter()
            .filter_map(|&(t, s, (rl, rs, ll))| d.update(SwingInput { now_ms: t, stamina: s, right_light: rl, right_strong: rs, left_light: ll }).map(|a| (t, a)))
            .collect()
    }

    const LIGHT_R: Attack = Attack { hand: Hand::Right, strong: false };
    const STRONG_R: Attack = Attack { hand: Hand::Right, strong: true };
    const LIGHT_L: Attack = Attack { hand: Hand::Left, strong: false };

    #[test]
    fn a_press_and_a_stamina_drop_is_an_attack() {
        let out = run(&[(0, 100, NONE), (16, 100, RL), (33, 82, RL), (50, 83, NONE)]);
        assert_eq!(out, vec![(33, LIGHT_R)]);
    }

    #[test]
    fn a_drop_without_a_press_is_not_an_attack() {
        // a roll: stamina falls 20, no attack input
        assert!(run(&[(0, 100, NONE), (16, 80, NONE)]).is_empty());
        // sprinting: a point or so at a time
        let sprint: Vec<_> = (0..60).map(|k| (k * 16, 100 - k as i32, NONE)).collect();
        assert!(run(&sprint).is_empty());
        // even with the button held down for the whole sprint: small steps never count
        let held: Vec<_> = (0..60).map(|k| (k * 16, 100 - k as i32, RL)).collect();
        assert!(run(&held).is_empty());
    }

    #[test]
    fn a_press_without_a_drop_is_not_an_attack() {
        assert!(run(&[(0, 0, NONE), (16, 0, RL), (32, 0, NONE)]).is_empty());
    }

    #[test]
    fn an_old_press_does_not_explain_a_late_drop() {
        let out = run(&[(0, 100, NONE), (10, 100, RL), (20, 100, NONE), (ATTACK_WINDOW_MS + 100, 80, NONE)]);
        assert!(out.is_empty());
    }

    #[test]
    fn the_input_says_which_hand_and_whether_it_is_strong() {
        assert_eq!(run(&[(0, 100, NONE), (16, 100, RS), (32, 70, RS)]), vec![(32, STRONG_R)]);
        // the left hand: the right mouse button / left shoulder button (a crossbow shot costs more stamina than a light attack)
        assert_eq!(run(&[(0, 100, NONE), (16, 100, LL), (32, 74, LL)]), vec![(32, LIGHT_L)]);
        // the most recent press wins when two were pressed
        let both = run(&[(0, 100, NONE), (10, 100, RS), (50, 100, (true, true, false)), (66, 80, (true, true, false))]);
        assert_eq!(both, vec![(66, LIGHT_R)]);
        let other_way = run(&[(0, 100, NONE), (10, 100, RL), (50, 100, (true, true, false)), (66, 80, (true, true, false))]);
        assert_eq!(other_way, vec![(66, STRONG_R)]);
    }

    #[test]
    fn one_press_gives_one_attack_and_a_second_drop_in_a_blink_is_ignored() {
        let out = run(&[(0, 100, NONE), (16, 100, RL), (32, 85, RL), (48, 70, RL)]);
        assert_eq!(out.len(), 1, "{out:?}");
    }

    #[test]
    fn two_presses_close_together_are_two_attacks_only_after_the_debounce() {
        // second press 100 ms after the first attack: too soon, ignored; 200 ms after: counts
        let soon = run(&[(0, 100, NONE), (10, 100, RL), (20, 85, RL), (30, 85, NONE), (120, 85, RL), (130, 70, RL)]);
        assert_eq!(soon.len(), 1, "{soon:?}");
        let later = run(&[(0, 100, NONE), (10, 100, RL), (20, 85, RL), (30, 85, NONE), (200, 85, RL), (215, 70, RL)]);
        assert_eq!(later.len(), 2, "{later:?}");
    }

    #[test]
    fn the_stamina_jump_of_a_respawn_does_not_trigger_anything() {
        // stamina going UP, or a fresh character starting from nothing, is never a drop
        assert!(run(&[(0, 0, RL), (16, 100, RL), (32, 100, RL)]).is_empty());
    }

    #[test]
    fn combos_step_through_the_light_sounds_and_start_over_after_a_pause() {
        let mut c = Combo::new();
        let mut steps = Vec::new();
        let mut t = 0;
        for _ in 0..6 {
            t += 600;
            steps.push(c.swing(false, t));
        }
        t += 3000; // a long pause ends the combo
        steps.push(c.swing(false, t));
        steps.push(c.swing(false, t + 700));
        let want: Vec<Swing> = [1u8, 2, 3, 4, 1, 2, 1, 2].iter().map(|&s| Swing::Light { step: s }).collect();
        assert_eq!(steps, want);
    }

    #[test]
    fn a_strong_attack_is_strong_and_keeps_the_combo_window_open() {
        let mut c = Combo::new();
        assert_eq!(c.swing(false, 0), Swing::Light { step: 1 });
        assert_eq!(c.swing(true, 600), Swing::Strong);
        assert_eq!(c.swing(false, 1200), Swing::Light { step: 2 }, "the strong attack did not break the combo");
        assert_eq!(c.swing(true, 5000), Swing::Strong);
        assert_eq!(c.swing(false, 5600), Swing::Light { step: 3 }, "the window was open again");
        assert_eq!(c.swing(false, 9000), Swing::Light { step: 1 });
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
    fn a_burst_of_three_taken_at_once_is_three_shots() {
        let mut a = AmmoTracker::new();
        a.update(Some(&[(404000, 60)]));
        assert_eq!(a.update(Some(&[(404000, 57)])), 3);
        assert_eq!(a.update(Some(&[(404000, 53)])), 0, "four at once is a drop or a sale, not a burst");
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
    fn a_burst_is_spread_out_but_a_single_shot_is_immediate() {
        let mut q = ShotQueue::new();
        q.add(3, 1000);
        assert_eq!(q.waiting(), 3);
        assert_eq!(q.take_due(1000), 1, "the first shot sounds at once");
        assert_eq!(q.take_due(1000 + SHOT_SPACING_MS - 1), 0);
        assert_eq!(q.take_due(1000 + SHOT_SPACING_MS), 1);
        assert_eq!(q.take_due(1000 + 2 * SHOT_SPACING_MS + 500), 1, "late frames catch up");
        assert_eq!(q.waiting(), 0);
        // one shot long after: immediate again
        q.add(1, 5000);
        assert_eq!(q.take_due(5000), 1);
    }

    #[test]
    fn bolts_seen_one_by_one_are_not_delayed_more_than_the_spacing() {
        // three bolts leave 80 ms apart: they are each played, the second and third slightly pushed back to keep the spacing
        let mut q = ShotQueue::new();
        let mut played = Vec::new();
        for (t, n) in [(1000u64, 1u32), (1080, 1), (1160, 1)] {
            q.add(n, t);
        }
        for t in (1000..1400).step_by(10) {
            for _ in 0..q.take_due(t) {
                played.push(t);
            }
        }
        assert_eq!(played, vec![1000, 1110, 1220]);
    }

    #[test]
    fn a_huge_count_is_capped() {
        let mut q = ShotQueue::new();
        q.add(50, 0);
        assert_eq!(q.waiting(), AmmoTracker::MAX_SHOTS_AT_ONCE as usize);
    }

    #[test]
    fn edges_fire_once_per_press() {
        let mut e = Edge::default();
        let seq = [false, true, true, false, true];
        let got: Vec<bool> = seq.iter().map(|&d| e.rising(d)).collect();
        assert_eq!(got, vec![false, true, false, false, true]);
    }
}
