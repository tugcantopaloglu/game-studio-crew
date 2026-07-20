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
}

impl Projection {
    pub fn total(&self) -> u64 {
        self.prefix_tokens + self.brief_tokens + self.output_reserve
    }

    pub fn projected_usd(&self, model: Model) -> f64 {
        let u = if self.prefix_is_warm {
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
        };
        usd_mirror(model, u)
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
}

impl Enforcer {
    pub fn new(task_limit: u64, sprint_limit: u64) -> Self {
        Self {
            task: Budget::new(task_limit),
            sprint: Budget::new(sprint_limit),
            applied: None,
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

        if need > self.sprint.remaining() {
            self.applied = Some(Step::HardStop);
            return Admission::Refuse {
                reason: format!(
                    "projected {need} tokens exceeds the {} left in the sprint budget",
                    self.sprint.remaining()
                ),
            };
        }

        if need > self.task.remaining() {
            self.applied = Some(Step::HardStop);
            return Admission::Refuse {
                reason: format!(
                    "projected {need} tokens exceeds the {} left in the task budget",
                    self.task.remaining()
                ),
            };
        }

        let (scope, b) = self.tightest();
        let after = (b.spent + need) as f64 / b.limit.max(1) as f64;
        if after >= WARN_AT {
            let step = match self.applied {
                None => Step::EffortDownshift,
                Some(s) => s.next().unwrap_or(Step::HardStop),
            };
            self.applied = Some(step);
            return Admission::Degrade {
                step,
                reason: format!(
                    "{:?} budget would reach {:.0}% after this spawn",
                    scope,
                    after * 100.0
                ),
            };
        }

        Admission::Admit
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
        }
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
    fn no_step_ever_routes_work_onto_fable() {
        for s in Step::LADDER {
            for role_model in [Model::Opus, Model::Fable, Model::Haiku] {
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
    fn approaching_the_limit_degrades_one_step_at_a_time() {
        let mut e = Enforcer::new(20_000, 1_000_000);
        e.record(14_000);

        match e.admit(proj(2000, 500, 500, true)) {
            Admission::Degrade { step, .. } => assert_eq!(step, Step::EffortDownshift),
            other => panic!("expected a first degrade, got {other:?}"),
        }
        match e.admit(proj(2000, 500, 500, true)) {
            Admission::Degrade { step, .. } => assert_eq!(step, Step::SummarizerDownshift),
            other => panic!("expected the second step, got {other:?}"),
        }
        assert_eq!(e.state(), BudgetState::Degrading);
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
