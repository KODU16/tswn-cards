//! Runtime 的机制执行器；子模块按行动、受击、效果与生命周期划分。
//!
//! [`CombatRuntime`] 组合实体、世界、注册表、handler、槽、队列和 RNG。
//! 调度器选行动，hook 读取当前数据并修改受击值或入队，效果提交后同步世界与消息。
//! 各攻击与状态效果使用各自分支，沿具体技能入口可查看完整的伤害与状态链。
//! 普通应用通过 [`super::RuntimeRunner`] 或规范 BattleSession 使用本执行器。

use super::*;

mod attack;
mod defense;
mod effects;
mod infection;
mod lifecycle;
mod passive;
mod round;
mod skills_control;
mod skills_debuff;
mod skills_lifecycle;
mod skills_offense;
mod skills_team;

#[derive(Debug, Clone)]
/// 单回合结果：分别记录行动、可选可见帧与当前胜者。
pub struct RoundOutcome {
    pub action: Option<ActionPlan>,
    pub frame: Option<RuntimeFrame>,
    pub winner_team: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedBuiltinSkill {
    pub skill: BuiltinActiveSkill,
    pub fixed_lane: usize,
}

/// 内置主动技能的已选目标；常规上限为地震的 6 个目标，直接保存在栈内。
pub type PreparedTargetList = smallvec::SmallVec<[EntityIdx; 8]>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedBuiltinSkillAction {
    pub selected: SelectedBuiltinSkill,
    pub targets: PreparedTargetList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainAttackOnDamage {
    None,
    Absorb,
    Berserk,
    Curse,
    Fire(u32),
    Ice,
    Poison,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedPlainAction {
    BasicAttack {
        target: EntityIdx,
        use_magic: bool,
        amount: i32,
    },
    ForcedAttack {
        target: EntityIdx,
        amount: i32,
    },
    Saitama {
        target: Option<EntityIdx>,
    },
    BuiltinSkill(PreparedBuiltinSkillAction),
}

#[derive(Debug, Clone)]
/// 一场战斗的完整可变机制容器；跨语言显示数据由会话层投影。
pub struct CombatRuntime {
    pub entities: EntityArena,
    pub world: WorldArena,
    pub scheduler: PhaseScheduler,
    pub effects: EffectQueue,
    pub effect_handlers: EffectHandlers,
    pub skill_handlers: SkillHandlers,
    pub state_handlers: StateHandlers,
    pub replay_renderers: ReplayRenderers,
    pub show_renderers: ShowRenderers,
    pub scratch: BattleScratch,
    pub template_slots: TemplateSlotStorage,
    pub slots: BattleSlotStorage,
    pub registry: ExtensionRegistry,
    pub rng: RC4,
    #[cfg(not(feature = "no_debug"))]
    pub trace: Option<RuntimeTrace>,
    pub round: u64,
    /// 仅网站遗物启用；普通引擎对局保持原始规则。
    pub relic_heal_team: Option<usize>,
    pub relic_shield_team: Option<usize>,
    pub relic_damage_floor: bool,
    /// 背刺遗物只作用于指定队伍，受击计数与潜行状态分离保存。
    pub assassinate_relic_team: Option<usize>,
    pub relic_assassinate_hits: std::collections::HashMap<u32, u8>,
    /// 遗物护盾与技能护盾分别累计，再在防御结算时共同吸收伤害。
    pub relic_shields: std::collections::HashMap<u32, i32>,
}

impl CombatRuntime {
    /// 戾气只拦截敌对双方的低额伤害，队友、自伤与回复不受影响。
    pub(crate) fn relic_blocks_damage(&self, caster: EntityIdx, target: EntityIdx, amount: i32) -> bool {
        self.relic_damage_floor && amount > 0 && amount < 30
            && self.entities.get(caster).zip(self.entities.get(target))
                .is_some_and(|(from, to)| from.runtime.team != to.runtime.team)
    }
}
