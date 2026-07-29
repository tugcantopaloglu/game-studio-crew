use serde::{Deserialize, Serialize};
use studio_context::Model;

pub const CACHE_READ_MULTIPLIER: f64 = 0.1;
pub const CACHE_WRITE_MULTIPLIER: f64 = 2.0;
pub const WARN_AT: f64 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Task,
    Sprint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetState {
    Ok,
    Warned,
    Degrading,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    EffortDownshift,
    SummarizerDownshift,
    TrimL3,
    ForceSummarize,
    HardStop,
}

impl Step {
    pub fn number(&self) -> u8 {
        match self {
            Step::EffortDownshift => 1,
            Step::SummarizerDownshift => 2,
            Step::TrimL3 => 3,
            Step::ForceSummarize => 4,
            Step::HardStop => 5,
        }
    }

    pub fn next(&self) -> Option<Step> {
        match self {
            Step::EffortDownshift => Some(Step::SummarizerDownshift),
            Step::SummarizerDownshift => Some(Step::TrimL3),
            Step::TrimL3 => Some(Step::ForceSummarize),
            Step::ForceSummarize => Some(Step::HardStop),
            Step::HardStop => None,
        }
    }

    pub const LADDER: [Step; 5] = [
        Step::EffortDownshift,
        Step::SummarizerDownshift,
        Step::TrimL3,
        Step::ForceSummarize,
        Step::HardStop,
    ];
}

pub fn price_per_mtok(model: Model) -> (f64, f64) {
    match model {
        Model::Fable => (10.0, 50.0),
        Model::Opus => (5.0, 25.0),
        Model::Sonnet => (3.0, 15.0),
        Model::Haiku => (1.0, 5.0),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

pub fn usd_mirror(model: Model, u: Usage) -> f64 {
    let (input, output) = price_per_mtok(model);
    (u.input as f64 * input
        + u.cache_read as f64 * input * CACHE_READ_MULTIPLIER
        + u.cache_creation as f64 * input * CACHE_WRITE_MULTIPLIER
        + u.output as f64 * output)
        / 1_000_000.0
}

pub fn charged_tokens(u: Usage) -> u64 {
    (u.input as f64 + u.output as f64 + u.cache_creation as f64 * CACHE_WRITE_MULTIPLIER).round()
        as u64
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    pub limit: u64,
    pub spent: u64,
}

impl Budget {
    pub fn new(limit: u64) -> Self {
        Self { limit, spent: 0 }
    }

    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.spent)
    }

    pub fn fraction(&self) -> f64 {
        if self.limit == 0 {
            1.0
        } else {
            self.spent as f64 / self.limit as f64
        }
    }

    pub fn state(&self, applied: Option<Step>) -> BudgetState {
        if self.spent >= self.limit {
            BudgetState::Stopped
        } else if applied.is_some() {
            BudgetState::Degrading
        } else if self.fraction() >= WARN_AT {
            BudgetState::Warned
        } else {
            BudgetState::Ok
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub prefix_tokens: u64,
    pub brief_tokens: u64,
    pub output_reserve: u64,
    pub prefix_is_warm: bool,
    pub node_reserve: u64,
}

impl Projection {
    pub fn opening_turn(&self) -> Usage {
        if self.prefix_is_warm {
            Usage {
                input: self.brief_tokens,
                output: self.output_reserve,
                cache_read: self.prefix_tokens,
                cache_creation: 0,
            }
        } else {
            Usage {
                input: self.brief_tokens,
                output: self.output_reserve,
                cache_read: 0,
                cache_creation: self.prefix_tokens,
            }
        }
    }

    pub fn total(&self) -> u64 {
        charged_tokens(self.opening_turn()).max(self.node_reserve)
    }

    pub fn projected_usd(&self, model: Model) -> f64 {
        usd_mirror(model, self.opening_turn())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Admission {
    Admit,
    Degrade { step: Step, reason: String },
    Refuse { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enforcer {
    pub task: Budget,
    pub sprint: Budget,
    pub applied: Option<Step>,
    pub reserved: u64,
}

impl Enforcer {
    pub fn new(task_limit: u64, sprint_limit: u64) -> Self {
        Self {
            task: Budget::new(task_limit),
            sprint: Budget::new(sprint_limit),
            applied: None,
            reserved: 0,
        }
    }

    pub fn record(&mut self, tokens: u64) {
        self.task.spent += tokens;
        self.sprint.spent += tokens;
    }

    pub fn tightest(&self) -> (Scope, Budget) {
        if self.sprint.fraction() >= self.task.fraction() {
            (Scope::Sprint, self.sprint)
        } else {
            (Scope::Task, self.task)
        }
    }

    pub fn state(&self) -> BudgetState {
        let (_, b) = self.tightest();
        b.state(self.applied)
    }

    pub fn admit(&mut self, p: Projection) -> Admission {
        let need = p.total();

        if self.applied == Some(Step::HardStop) {
            return Admission::Refuse {
                reason: "the scope is hard stopped; no new workers spawn".into(),
            };
        }

        if need > self.unreserved(self.sprint) {
            self.applied = Some(Step::HardStop);
            return Admission::Refuse {
                reason: format!(
                    "projected {need} tokens exceeds the {} left in the sprint budget",
                    self.unreserved(self.sprint)
                ),
            };
        }

        if need > self.unreserved(self.task) {
            self.applied = Some(Step::HardStop);
            return Admission::Refuse {
                reason: format!(
                    "projected {need} tokens exceeds the {} left in the task budget",
                    self.unreserved(self.task)
                ),
            };
        }

        let (scope, b) = self.tightest();
        let after = (b.spent + self.reserved + need) as f64 / b.limit.max(1) as f64;
        if after >= WARN_AT {
            let step = match self.applied {
                None => step_for_pressure(after),
                Some(held) => held.max(step_for_pressure(after)),
            };
            self.applied = Some(step);
            self.reserved += need;
            return Admission::Degrade {
                step,
                reason: format!(
                    "{:?} budget would reach {:.0}% after this spawn",
                    scope,
                    after * 100.0
                ),
            };
        }

        self.reserved += need;
        Admission::Admit
    }

    fn unreserved(&self, b: Budget) -> u64 {
        b.remaining().saturating_sub(self.reserved)
    }

    pub fn release(&mut self, tokens: u64) {
        self.reserved = self.reserved.saturating_sub(tokens);
    }
}

pub fn step_for_pressure(after: f64) -> Step {
    let into_the_margin = (after - WARN_AT) / (1.0 - WARN_AT);
    if into_the_margin >= 0.75 {
        Step::ForceSummarize
    } else if into_the_margin >= 0.5 {
        Step::TrimL3
    } else if into_the_margin >= 0.25 {
        Step::SummarizerDownshift
    } else {
        Step::EffortDownshift
    }
}

pub fn ladder_saves_money(step: Step) -> bool {
    !matches!(step, Step::HardStop)
}

pub fn model_for_step(step: Step, role_model: Model, is_summarizer: bool) -> Model {
    match step {
        Step::SummarizerDownshift if is_summarizer => Model::Haiku,
        _ => role_model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proj(prefix: u64, brief: u64, reserve: u64, warm: bool) -> Projection {
        Projection {
            prefix_tokens: prefix,
            brief_tokens: brief,
            output_reserve: reserve,
            prefix_is_warm: warm,
            node_reserve: 0,
        }
    }

    #[test]
    fn a_projection_is_counted_the_way_the_ledger_will_charge_it() {
        let cold = proj(100_000, 500, 2_000, false);
        let warm = proj(100_000, 500, 2_000, true);

        assert_eq!(cold.total(), charged_tokens(cold.opening_turn()));
        assert_eq!(warm.total(), charged_tokens(warm.opening_turn()));
        assert!(
            cold.total() > warm.total(),
            "the same prefix is a cache write at {CACHE_WRITE_MULTIPLIER}x cold and a re-read of \
             tokens already paid for warm; adding raw token counts prices both at 1x and cannot \
             tell the expensive spawn from the cheap one"
        );
    }

    #[test]
    fn a_node_reserves_what_it_will_spend_rather_than_what_its_first_turn_costs() {
        let mut opening = proj(1_000, 173, 2_000, true);
        assert!(
            opening.total() < 10_000,
            "an opening turn against a warm prefix is small: {}",
            opening.total()
        );

        opening.node_reserve = 120_000;
        assert_eq!(
            opening.total(),
            120_000,
            "a node is a worker session of many turns and every turn re-reads the history it has \
             piled up; a run that reserved only the opening prompt settled 43% past its ceiling \
             before any gate refused it"
        );

        let hungry = Projection { node_reserve: 1_000, ..proj(400_000, 500, 2_000, false) };
        assert!(
            hungry.total() > 1_000,
            "the reserve is a floor under the estimate, never a cap on it"
        );
    }

    #[test]
    fn a_whole_wave_admitted_at_once_cannot_all_spend_the_same_remaining_budget() {
        let mut e = Enforcer::new(1_440_000, 1_440_000);
        let node = Projection { node_reserve: 400_000, ..proj(1_000, 200, 2_000, true) };

        let wave: Vec<Admission> = (0..4).map(|_| e.admit(node)).collect();
        assert!(
            matches!(wave[3], Admission::Refuse { .. }),
            "four workers run at once and are admitted before any of them charges, so the fourth \
             has to be told there is no room: {:?}",
            wave[3]
        );

        assert_eq!(
            e.reserved, 1_200_000,
            "the three that were let through are holding their reservations; only the refused \
             one took nothing"
        );
        assert_eq!(
            e.applied,
            Some(Step::HardStop),
            "a wave that does not fit is a run that cannot finish its plan, not a queue to retry"
        );
    }

    #[test]
    fn a_reservation_is_given_back_at_what_it_was_held_for_not_at_what_was_spent() {
        let mut e = Enforcer::new(1_000_000, 1_000_000);
        let node = Projection { node_reserve: 300_000, ..proj(0, 0, 0, true) };
        assert!(matches!(e.admit(node), Admission::Admit));
        assert_eq!(e.reserved, 300_000);

        e.release(300_000);
        e.record(450_000);
        assert_eq!(e.reserved, 0, "the hold is released whole; the overspend lands in spent");
        assert_eq!(e.sprint.remaining(), 550_000);
    }

    #[test]
    fn the_ladder_has_five_steps_in_the_documented_order() {
        assert_eq!(Step::LADDER.len(), 5);
        for (i, s) in Step::LADDER.iter().enumerate() {
            assert_eq!(s.number() as usize, i + 1);
        }
        assert_eq!(Step::EffortDownshift.next(), Some(Step::SummarizerDownshift));
        assert_eq!(Step::ForceSummarize.next(), Some(Step::HardStop));
        assert_eq!(Step::HardStop.next(), None);
    }

    #[test]
    fn only_the_last_step_stops_making_progress() {
        for s in Step::LADDER {
            assert_eq!(ladder_saves_money(s), s != Step::HardStop);
        }
    }

    #[test]
    fn the_summarizer_downshifts_to_the_cheapest_tier_not_the_middle_one() {
        let chosen = model_for_step(Step::SummarizerDownshift, Model::Opus, true);
        assert_eq!(
            chosen,
            Model::Haiku,
            "this step exists to cut spend under budget pressure: haiku is $1/$5 against \
             sonnet's $3/$15, so downshifting to sonnet would save 40% off opus where haiku \
             saves 80%. Nobody has measured sonnet against haiku on rollup quality, and the \
             ladder already falls back to a zero-token template when a rollup is unusable, so \
             the cheapest tier stays the target until a measurement says otherwise."
        );
        assert_ne!(chosen, Model::Sonnet);
    }

    #[test]
    fn every_model_is_checked_against_the_ladder_including_ones_added_later() {
        assert_eq!(
            Model::ALL.len(),
            4,
            "a new model variant must be swept through the ladder, not just added to the enum"
        );
    }

    #[test]
    fn no_step_ever_routes_work_onto_fable() {
        for s in Step::LADDER {
            for role_model in Model::ALL {
                for summarizer in [true, false] {
                    let chosen = model_for_step(s, role_model, summarizer);
                    if role_model != Model::Fable {
                        assert_ne!(
                            chosen,
                            Model::Fable,
                            "step {s:?} moved work onto fable, which costs twice opus"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_summarizer_step_only_downshifts_the_summarizer() {
        assert_eq!(
            model_for_step(Step::SummarizerDownshift, Model::Opus, true),
            Model::Haiku
        );
        assert_eq!(
            model_for_step(Step::SummarizerDownshift, Model::Opus, false),
            Model::Opus
        );
        assert_eq!(
            model_for_step(Step::SummarizerDownshift, Model::Fable, false),
            Model::Fable,
            "tier 1 stays on fable regardless of budget state"
        );
    }

    #[test]
    fn a_warm_prefix_is_far_cheaper_than_a_cold_one() {
        let cold = proj(8867, 0, 0, false).projected_usd(Model::Opus);
        let warm = proj(8867, 0, 0, true).projected_usd(Model::Opus);
        assert!((cold - 0.0887).abs() < 0.001, "cold was {cold}");
        assert!((warm - 0.0044).abs() < 0.001, "warm was {warm}");
        assert!(cold / warm > 15.0, "the measured gap is about 20x on the prefix alone");
    }

    #[test]
    fn the_usd_mirror_matches_the_measured_invocation() {
        let u = Usage { input: 2, output: 4, cache_read: 0, cache_creation: 8867 };
        let usd = usd_mirror(Model::Opus, u);
        assert!((usd - 0.0888).abs() < 0.0005, "expected the measured 0.0888, got {usd}");
    }

    #[test]
    fn fable_costs_twice_opus_on_both_sides() {
        let (fi, fo) = price_per_mtok(Model::Fable);
        let (oi, oo) = price_per_mtok(Model::Opus);
        assert_eq!(fi, oi * 2.0);
        assert_eq!(fo, oo * 2.0);
    }

    #[test]
    fn a_comfortable_spawn_is_admitted_untouched() {
        let mut e = Enforcer::new(100_000, 1_000_000);
        assert_eq!(e.admit(proj(9000, 4000, 8000, true)), Admission::Admit);
        assert_eq!(e.state(), BudgetState::Ok);
    }

    #[test]
    fn the_rung_follows_the_pressure_rather_than_the_number_of_spawns() {
        let mut e = Enforcer::new(100_000, 1_000_000);
        e.record(75_000);

        match e.admit(proj(1000, 500, 500, true)) {
            Admission::Degrade { step, .. } => assert_eq!(step, Step::EffortDownshift),
            other => panic!("expected the first rung, got {other:?}"),
        }

        for _ in 0..20 {
            let one = proj(1000, 500, 500, true);
            match e.admit(one) {
                Admission::Degrade { step, .. } => assert_eq!(
                    step,
                    Step::EffortDownshift,
                    "the ladder must not walk itself to a hard stop just because many \
                     nodes were admitted at the same pressure"
                ),
                other => panic!("expected a degrade, got {other:?}"),
            }
            e.release(one.total());
        }
        assert_eq!(e.state(), BudgetState::Degrading);
    }

    #[test]
    fn a_tighter_budget_climbs_to_a_deeper_rung() {
        let mut e = Enforcer::new(100_000, 1_000_000);
        e.record(96_000);
        match e.admit(proj(1000, 500, 500, true)) {
            Admission::Degrade { step, .. } => assert_eq!(step, Step::ForceSummarize),
            other => panic!("expected the deepest saving rung, got {other:?}"),
        }
    }

    #[test]
    fn pressure_alone_never_hard_stops_a_run_that_can_still_afford_the_spawn() {
        let mut e = Enforcer::new(100_000, 1_000_000);
        e.record(80_000);
        for _ in 0..50 {
            let one = proj(1000, 500, 500, true);
            assert!(
                !matches!(e.admit(one), Admission::Refuse { .. }),
                "a hard stop must mean the budget is out, not that it degraded five times"
            );
            e.release(one.total());
        }
        assert_ne!(e.applied, Some(Step::HardStop));
    }

    #[test]
    fn a_rung_once_reached_is_never_walked_back() {
        let mut e = Enforcer::new(100_000, 1_000_000);
        e.record(96_000);
        assert!(matches!(e.admit(proj(1000, 500, 500, true)), Admission::Degrade { .. }));
        let deep = e.applied;
        e.task.limit = 1_000_000;
        e.sprint.limit = 1_000_000;
        if let Admission::Degrade { step, .. } = e.admit(proj(1000, 500, 500, true)) {
            assert_eq!(Some(step), deep);
        }
        assert_eq!(e.applied, deep);
    }

    #[test]
    fn a_spawn_that_cannot_fit_is_refused_before_any_tokens_are_paid() {
        let mut e = Enforcer::new(10_000, 1_000_000);
        e.record(9_000);
        match e.admit(proj(9000, 4000, 8000, false)) {
            Admission::Refuse { reason } => assert!(reason.contains("task budget")),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_sprint_budget_refuses_before_the_task_budget_does() {
        let mut e = Enforcer::new(1_000_000, 10_000);
        e.record(9_500);
        match e.admit(proj(2000, 500, 500, true)) {
            Admission::Refuse { reason } => assert!(reason.contains("sprint budget")),
            other => panic!("expected the sprint to refuse, got {other:?}"),
        }
    }

    #[test]
    fn once_hard_stopped_nothing_else_is_admitted() {
        let mut e = Enforcer::new(1_000, 1_000);
        e.record(999);
        assert!(matches!(e.admit(proj(500, 100, 100, true)), Admission::Refuse { .. }));
        assert!(matches!(e.admit(proj(1, 0, 0, true)), Admission::Refuse { .. }));
        assert_eq!(e.applied, Some(Step::HardStop));
    }

    #[test]
    fn the_tightest_scope_drives_the_decision() {
        let mut e = Enforcer::new(100_000, 20_000);
        e.record(18_000);
        assert_eq!(e.tightest().0, Scope::Sprint);

        let mut f = Enforcer::new(20_000, 1_000_000);
        f.record(18_000);
        assert_eq!(f.tightest().0, Scope::Task);
    }

    #[test]
    fn spending_is_recorded_against_both_scopes() {
        let mut e = Enforcer::new(100, 200);
        e.record(50);
        assert_eq!(e.task.spent, 50);
        assert_eq!(e.sprint.spent, 50);
        assert_eq!(e.task.remaining(), 50);
        assert_eq!(e.sprint.remaining(), 150);
    }

    #[test]
    fn a_budget_at_its_limit_reports_stopped() {
        let b = Budget { limit: 100, spent: 100 };
        assert_eq!(b.state(None), BudgetState::Stopped);
        let warned = Budget { limit: 100, spent: 80 };
        assert_eq!(warned.state(None), BudgetState::Warned);
        let ok = Budget { limit: 100, spent: 10 };
        assert_eq!(ok.state(None), BudgetState::Ok);
    }
}

#[cfg(test)]
mod charged_tests {
    use super::*;

    #[test]
    fn a_cache_read_costs_the_ceiling_nothing_because_the_run_already_paid_for_it() {
        let cached = Usage { input: 0, output: 0, cache_read: 1000, cache_creation: 0 };
        let fresh = Usage { input: 1000, output: 0, cache_read: 0, cache_creation: 0 };
        assert_eq!(charged_tokens(cached), 0);
        assert_eq!(charged_tokens(fresh), 1000);
    }

    #[test]
    fn a_cache_write_still_carries_its_full_premium() {
        let cold = Usage { input: 500, output: 800, cache_read: 0, cache_creation: 30_000 };
        assert_eq!(charged_tokens(cold), 61_300);
    }

    #[test]
    fn a_warm_worker_is_far_cheaper_against_the_budget_than_a_cold_one() {
        let warm = Usage { input: 500, output: 800, cache_read: 30_000, cache_creation: 0 };
        let cold = Usage { input: 500, output: 800, cache_read: 0, cache_creation: 30_000 };

        assert!(
            charged_tokens(warm) * 10 < charged_tokens(cold),
            "warm {} should be an order of magnitude under cold {}",
            charged_tokens(warm),
            charged_tokens(cold)
        );
    }

    #[test]
    fn re_reading_a_prefix_every_turn_cannot_walk_a_node_past_its_own_budget() {
        let long_session = Usage {
            input: 5_409,
            output: 15_000,
            cache_read: 2_741_680,
            cache_creation: 0,
        };

        assert_eq!(charged_tokens(long_session), 20_409);
        assert!(
            charged_tokens(long_session) < 120_000,
            "this is the game_designer node that ended a twelve-task build at step five: 20,409 \
             tokens of real traffic charged as 294,577 because every turn re-read the history the \
             last one grew, so one node ate 2.45x the 120,000 its plan gave it while the dollar \
             meter still read $1.88 of $25"
        );
    }

    #[test]
    fn the_dollar_mirror_still_prices_every_cache_read_it_is_charged_for() {
        let u = Usage { input: 0, output: 0, cache_read: 1_000_000, cache_creation: 0 };
        assert_eq!(charged_tokens(u), 0);
        assert!(
            usd_mirror(Model::Opus, u) > 0.0,
            "the ceiling stops counting re-reads; the price of them is still real and still shown"
        );
    }
}
