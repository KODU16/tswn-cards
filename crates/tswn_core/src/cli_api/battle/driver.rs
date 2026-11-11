use super::{BattleOptions, BattleStopReason};
use crate::cli_api::{CliApiError, CliApiResult, invalid_input};
use crate::runtime::model_state::{BattleModelState, ModelStateError};
use crate::runtime::{RunUpdates, RuntimeRunner, StateEntry};
use crate::runtime::profile::{DEFAULT_CORE_HASTE_STATE_EXPORT, DEFAULT_CORE_SLOW_STATE_EXPORT, PLAIN_HASTE_STATE_KEY};

/// 两种会话共享的逐回合驱动；展示层仍在每个空回合后更新差分快照。
#[derive(Debug)]
pub(super) struct BattleDriver {
    pub runner: RuntimeRunner,
    /// cards 的队列和暂停标记必须属于会话，不能放在全局 TLS 中长期保存。
    pub forced: crate::runtime::forced_action::ForcedState,
    pub rounds_advanced: usize,
    pub frames_emitted: usize,
    pub stop_reason: Option<BattleStopReason>,
    pub failure: Option<CliApiError>,
    max_rounds: usize,
    no_progress_rounds: usize,
}

pub(super) struct RoundStep {
    pub updates: RunUpdates,
    pub round_index: usize,
    pub frame_index: Option<usize>,
}

impl BattleDriver {
    pub fn from_raw(raw: &str, options: BattleOptions) -> CliApiResult<Self> {
        if raw.trim().is_empty() {
            return Err(invalid_input("raw_input is empty"));
        }
        if options.max_rounds == 0 {
            return Err(CliApiError::InvalidArgument("battle max_rounds must be positive".into()));
        }
        if !options.eval_rq.is_finite() {
            return Err(CliApiError::InvalidArgument("battle eval_rq must be finite".into()));
        }
        let (groups, seed) = RuntimeRunner::split_namerena_into_groups(raw.to_owned());
        let mut runner = RuntimeRunner::new_from_groups_with_seed_and_eval_rq(&groups, &seed, options.eval_rq)
            .map_err(|e| CliApiError::RunnerInit(e.to_string()))?;
        {
            let runtime = runner.runtime_mut();
            runtime.relic_heal_team = options.healing_relic_team;
            runtime.relic_shield_team = options.shielding_relic_team;
            runtime.relic_damage_floor = options.relic_damage_floor;
            runtime.assassinate_relic_team = options.assassinate_relic_team;
        }
        if let Some(team) = options.stat_relic_team {
            // 加成只落在开局原始角色的运行时，分身和召唤模板不会继承该遗物。
            let ids: Vec<_> = runner.runtime().entities.iter()
                .filter(|(_, entity)| entity.runtime.alive && entity.runtime.team == team && entity.runtime.owner == entity.runtime.root_owner)
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                let first = runner.runtime_mut().rng.next_u8() as usize % 8;
                let mut second = runner.runtime_mut().rng.next_u8() as usize % 8;
                while second == first {
                    second = runner.runtime_mut().rng.next_u8() as usize % 8;
                }
                let entity = runner.runtime_mut().entities.get_mut(id).expect("relic target disappeared");
                for field in [first, second] {
                    match field {
                        0 => entity.runtime.attack += 10,
                        1 => entity.runtime.defense += 10,
                        2 => entity.runtime.speed += 10,
                        3 => entity.runtime.agility += 10,
                        4 => entity.runtime.magic += 10,
                        5 => entity.runtime.resistance += 10,
                        6 => entity.runtime.wisdom += 10,
                        7 => { entity.template.max_hp += 30; entity.runtime.hp += 30; },
                        _ => unreachable!(),
                    }
                }
            }
        }
        if let Some(team) = options.opening_relic_team {
            let runtime = runner.runtime_mut();
            let haste_id = runtime.registry.state_id_by_export_name(DEFAULT_CORE_HASTE_STATE_EXPORT)
                .expect("default profile must register haste");
            let slow_id = runtime.registry.state_id_by_export_name(DEFAULT_CORE_SLOW_STATE_EXPORT)
                .expect("default profile must register slow");
            let haste_priority = runtime.registry.state(haste_id).expect("haste state disappeared").priority;
            let slow_priority = runtime.registry.state(slow_id).expect("slow state disappeared").priority;
            // 沿用技能本身的状态键与持续计数，开场快照和行动顺序立即反映遗物效果。
            for entity in runtime.entities.iter_all_mut().filter(|entity| entity.runtime.alive) {
                if entity.runtime.team == team {
                    entity.states.add_entry(StateEntry::haste(PLAIN_HASTE_STATE_KEY, haste_id, 2, 1, haste_priority));
                } else {
                    entity.states.add_entry(StateEntry::slow(78, slow_id, 1, slow_priority));
                }
            }
        }
        if let Some(pct) = options.start_hp_pct {
            // 百分比开局生命只服务 cards 的受控队伍；无队伍时不得悄悄改双方状态。
            if !pct.is_finite() || pct <= 0.0 || (pct > 1.0 && options.controlled_team.is_none()) {
                return Err(CliApiError::InvalidArgument("start_hp_pct must be finite and positive; bonuses require controlled_team".into()));
            }
            let team = options.controlled_team;
            let runtime = runner.runtime_mut();
            for entity in runtime.entities.iter_all_mut() {
                let is_ours = match team {
                    Some(t) => entity.runtime.team == t,
                    None => true,
                };
                if is_ours && entity.runtime.alive {
                    entity.runtime.hp = (entity.template.max_hp as f64 * pct).round() as i32;
                }
            }
        }
        let mut forced = crate::runtime::forced_action::take_pending_actions();
        forced.controlled_team = options.controlled_team;
        Self::new_with_forced(runner, options.max_rounds, forced)
    }

    pub fn new(runner: RuntimeRunner, max_rounds: usize) -> CliApiResult<Self> {
        Self::new_with_forced(runner, max_rounds, crate::runtime::forced_action::ForcedState::default())
    }

    fn new_with_forced(
        runner: RuntimeRunner,
        max_rounds: usize,
        forced: crate::runtime::forced_action::ForcedState,
    ) -> CliApiResult<Self> {
        if max_rounds == 0 {
            return Err(CliApiError::InvalidArgument("battle max_rounds must be positive".into()));
        }
        Ok(Self {
            stop_reason: runner.have_winner().then_some(BattleStopReason::Winner),
            runner,
            forced,
            max_rounds,
            rounds_advanced: 0,
            frames_emitted: 0,
            no_progress_rounds: 0,
            failure: None,
        })
    }

    pub fn model_state(&self) -> Result<BattleModelState, ModelStateError> {
        if let Some(error) = &self.failure {
            return Err(ModelStateError(error.to_string()));
        }
        self.runner.model_state()
    }

    pub fn advance_round(&mut self) -> CliApiResult<Option<RoundStep>> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if self.stop_reason.is_some() {
            return Ok(None);
        }
        if let Err(error) = self.runner.validate_ready() {
            let error = CliApiError::Runtime(error.to_string());
            self.failure = Some(error.clone());
            return Err(error);
        }
        let round_index = self.rounds_advanced;
        // 暂停不能消耗行动条或随机流；补交指令后需要重放同一待行动作。
        let checkpoint = self.forced.controlled_team.map(|_| (self.runner.clone(), self.forced.clone()));
        let updates = crate::runtime::forced_action::with_state(&mut self.forced, || self.runner.main_round());
        if self.forced.paused_actor.is_some() {
            if let Some((runner, forced)) = checkpoint {
                let paused_actor = self.forced.paused_actor;
                self.runner = runner;
                self.forced = forced;
                self.forced.paused_actor = paused_actor;
            }
            return Ok(None);
        }
        self.rounds_advanced += 1;
        let visible = !updates.updates.is_empty() || self.runner.have_winner();
        self.no_progress_rounds = if visible { 0 } else { self.no_progress_rounds + 1 };
        self.stop_reason = if self.runner.have_winner() {
            Some(BattleStopReason::Winner)
        } else if self.rounds_advanced >= self.max_rounds {
            Some(BattleStopReason::MaxRounds)
        } else if self.no_progress_rounds >= self.runner.runtime.entities.iter().count().max(1).saturating_mul(16) {
            Some(BattleStopReason::NoProgress)
        } else {
            None
        };
        let frame_index = visible.then(|| {
            let index = self.frames_emitted;
            self.frames_emitted += 1;
            index
        });
        Ok(Some(RoundStep {
            updates,
            round_index,
            frame_index,
        }))
    }
}
