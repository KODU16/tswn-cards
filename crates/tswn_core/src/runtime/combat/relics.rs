//! 卡牌遗物状态持续、状态返还与阵亡吞噬；所有效果按队伍隔离。
use super::*;
impl CombatRuntime {
    // 仅在负面状态实际落地后调用；狂暴、净化及失败的命中不会进入此入口。
    pub(crate) fn language_debuff(&mut self, caster: EntityIdx, target: EntityIdx) {
        let Some(team) = self.language_relic_team else {
            return;
        };
        let Some(from) = self.entities.get(caster) else {
            return;
        };
        let root = self.entities.get(from.runtime.root_owner).unwrap_or(from);
        if root.template.team != team || from.runtime.team != team {
            return;
        }
        let extra = self.relic_debuff_turns(caster, target);
        let Some(to) = self.entities.get_mut(target) else {
            return;
        };
        if to.template.team == team || !to.runtime.alive || to.runtime.hp <= 0 {
            return;
        }
        let step = to
            .states
            .entry(PLAIN_BERSERK_STATE_KEY)
            .and_then(|entry| match entry.payload {
                StatePayload::Berserk { step } => Some(step),
                _ => None,
            })
            .unwrap_or(0)
            .max(1 + extra);
        if !to.states.set_payload(PLAIN_BERSERK_STATE_KEY, StatePayload::Berserk { step }) {
            to.states.add_entry(StateEntry::berserk(PLAIN_BERSERK_STATE_KEY, step));
        }
        self.relic_status_applied(target, PLAIN_BERSERK_STATE_KEY);
    }

    /// 敌对施法者与受击者的遗物可叠加，各增加一个行动周期。
    pub(crate) fn relic_debuff_turns(&self, caster: EntityIdx, target: EntityIdx) -> i32 {
        let Some(to) = self.entities.get(target) else {
            return 0;
        };
        let hostile = self
            .entities
            .get(caster)
            .is_some_and(|from| from.runtime.team != to.runtime.team && self.debuff_relic_team == Some(from.runtime.team));
        i32::from(hostile) + i32::from(self.thanks_relic_team == Some(to.template.team))
    }

    /// 状态实际施加成功后登记拜谢；永久负面状态没有自然倒计时。
    pub(crate) fn relic_status_applied(&mut self, target: EntityIdx, key: u32) {
        // 新施加的状态采用自己的持续时间，不能被旧返还的时限提前清除。
        self.relic_expiries.remove(&(target.0, key));
        let Some(entity) = self.entities.get(target) else {
            return;
        };
        if self.thanks_relic_team != Some(entity.template.team) {
            return;
        }
        if let Some(entry) = entity.states.entry(key) {
            self.relic_thanks.insert((target.0, key), entry.clone());
        }
    }

    /// 计时状态按各自内部步长转换，返还保留原强度但固定两个行动周期。
    fn relic_set_turns(payload: &mut StatePayload, turns: i32, target: EntityIdx) {
        match payload {
            StatePayload::Ice { frozen_step } => *frozen_step = turns * 1024,
            StatePayload::Poison {
                count,
                target: recipient,
                ..
            } => {
                *count = turns;
                *recipient = Some(target.0);
            }
            StatePayload::Slow { step } => *step = turns,
            StatePayload::Berserk { step } | StatePayload::Charm { step, .. } => *step = turns,
            _ => {}
        }
    }

    /// 只在实际状态消失后返还，不经命中或免疫骰，也不再触发遗物递归。
    pub(crate) fn relic_reflect_expired(&mut self, updates: &mut RunUpdates) {
        let mut expired: Vec<_> = self
            .relic_thanks
            .iter()
            .filter(|((id, key), _)| self.entities.get(EntityIdx(*id)).is_none_or(|e| e.states.entry(*key).is_none()))
            .map(|(key, value)| (*key, value.clone()))
            .collect();
        // HashMap 的遍历顺序不可影响同种子战报。
        expired.sort_by_key(|(key, _)| *key);
        for ((owner, key), entry) in expired {
            self.relic_thanks.remove(&(owner, key));
            let Some(source) = self.entities.get(EntityIdx(owner)) else {
                continue;
            };
            if source.runtime.hp <= 0 {
                continue;
            }
            let team = source.template.team;
            let targets: Vec<_> = self
                .entities
                .iter()
                .filter(|(_, e)| e.runtime.alive && e.runtime.hp > 0 && e.runtime.team != team)
                .map(|(id, _)| id)
                .collect();
            for target in targets {
                let mut reflected = entry.clone();
                Self::relic_set_turns(&mut reflected.payload, 2, target);
                // 魅惑返还时归属拜谢方，不能沿用原敌方施法者的阵营。
                if let StatePayload::Charm {
                    group_id,
                    effective_team_idx,
                    source_team_idx,
                    target: recipient,
                    ..
                } = &mut reflected.payload
                {
                    *group_id = owner as usize;
                    *effective_team_idx = Some(team);
                    *source_team_idx = Some(team);
                    *recipient = Some(target.0);
                }
                let entity = self.entities.get_mut(target).unwrap();
                if !entity.states.set_payload(key, reflected.payload.clone()) {
                    entity.states.add_entry(reflected);
                }
                entity.refresh_runtime_stats_from_template();
                self.relic_expiries.insert((target.0, key), 2);
                updates.add(RuntimeFrame::replay_update(
                    owner as usize,
                    target.0 as usize,
                    "[0]的[拜谢]返还了负面状态",
                    0,
                ));
            }
        }
    }

    /// 无自然倒计时的燃烧与诅咒，返还后也严格限制为两次受影响角色的行动。
    pub(crate) fn relic_tick_expiries(&mut self, actor: EntityIdx) {
        let keys: Vec<_> = self.relic_expiries.keys().filter(|(id, _)| *id == actor.0).copied().collect();
        for key in keys {
            let left = self.relic_expiries.get_mut(&key).unwrap();
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.relic_expiries.remove(&key);
                if let Some(entity) = self.entities.get_mut(actor) {
                    entity.states.clear_legacy_key(key.1);
                    entity.refresh_runtime_stats_from_template();
                }
            }
        }
    }

    /// 每次真实阵亡最多触发一次，复活后允许下一次阵亡重新触发。
    pub(crate) fn relic_inequality_death(&mut self, dead: EntityIdx, updates: &mut RunUpdates) {
        let Some(entity) = self.entities.get(dead) else {
            return;
        };
        if self.inequality_relic_team != Some(entity.template.team) || !self.relic_consumed.insert(dead.0) {
            return;
        }
        let team = entity.template.team;
        let targets: Vec<_> = self
            .entities
            .iter()
            .filter(|(id, e)| {
                *id != dead && e.runtime.alive && e.runtime.hp > 0 && e.template.team == team && e.runtime.owner == *id
            })
            .map(|(id, _)| id)
            .collect();
        if let Some(index) = self.rng.pick(&targets) {
            self.apply_plain_merge_into(targets[index], dead, updates);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> CombatRuntime {
        CombatRuntime::from_template(PreparedCombatTemplate::new(vec![
            PlayerTemplate::new(1, "player", 0, 1000, 100),
            PlayerTemplate::new(2, "enemy", 1, 1000, 100),
            PlayerTemplate::new(3, "ally", 0, 1000, 100),
        ]))
    }

    // 己方拜谢和敌方属性修改器同时生效，队伍之外没有持续加成。
    #[test]
    fn debuff_relics_stack_and_default_is_isolated() {
        let mut runtime = fixture();
        assert_eq!(runtime.relic_debuff_turns(EntityIdx(1), EntityIdx(0)), 0);
        runtime.debuff_relic_team = Some(1);
        runtime.thanks_relic_team = Some(0);
        assert_eq!(runtime.relic_debuff_turns(EntityIdx(1), EntityIdx(0)), 2);
        assert_eq!(runtime.relic_debuff_turns(EntityIdx(0), EntityIdx(1)), 0);
    }

    // 语言包在成功入口直接施加一回合狂暴，不叠加已有狂暴、不污染友方。
    #[test]
    fn language_relic_is_hostile_and_non_stacking() {
        let mut runtime = fixture();
        runtime.language_relic_team = Some(0);
        runtime.language_debuff(EntityIdx(0), EntityIdx(1));
        runtime.language_debuff(EntityIdx(0), EntityIdx(1));
        assert_eq!(
            runtime.entities.get(EntityIdx(1)).unwrap().states.entry(10).unwrap().payload,
            StatePayload::Berserk { step: 1 }
        );
        runtime.language_debuff(EntityIdx(0), EntityIdx(2));
        assert!(runtime.entities.get(EntityIdx(2)).unwrap().states.entry(10).is_none());
    }

    // 拜谢直到原状态消失才返还，返还状态严格持续两次行动且不会递归。
    #[test]
    fn thanks_returns_expired_status_for_two_actions() {
        let mut runtime = fixture();
        runtime.thanks_relic_team = Some(0);
        runtime
            .entities
            .get_mut(EntityIdx(0))
            .unwrap()
            .states
            .add_entry(StateEntry::berserk(10, 2));
        runtime.relic_status_applied(EntityIdx(0), 10);
        let mut updates = RunUpdates::new();
        runtime.relic_reflect_expired(&mut updates);
        assert!(runtime.entities.get(EntityIdx(1)).unwrap().states.entry(10).is_none());
        runtime.entities.get_mut(EntityIdx(0)).unwrap().states.clear_legacy_key(10);
        runtime.relic_reflect_expired(&mut updates);
        assert_eq!(
            runtime.entities.get(EntityIdx(1)).unwrap().states.entry(10).unwrap().payload,
            StatePayload::Berserk { step: 2 }
        );
        runtime.relic_tick_expiries(EntityIdx(1));
        assert!(runtime.entities.get(EntityIdx(1)).unwrap().states.entry(10).is_some());
        runtime.relic_tick_expiries(EntityIdx(1));
        assert!(runtime.entities.get(EntityIdx(1)).unwrap().states.entry(10).is_none());
        assert!(runtime.relic_thanks.is_empty());
    }

    // 不等式只选择存活的原始友方；重复死亡通知不能重复吞噬资源。
    #[test]
    fn inequality_consumes_once_and_excludes_enemy() {
        let mut runtime = fixture();
        runtime.inequality_relic_team = Some(0);
        runtime.entities.get_mut(EntityIdx(2)).unwrap().runtime.magic_point = 100;
        runtime.entities.get_mut(EntityIdx(2)).unwrap().runtime.hp = 0;
        let mut updates = RunUpdates::new();
        runtime.relic_inequality_death(EntityIdx(2), &mut updates);
        assert_eq!(runtime.entities.get(EntityIdx(0)).unwrap().runtime.magic_point, 100);
        assert_eq!(runtime.entities.get(EntityIdx(1)).unwrap().runtime.magic_point, 0);
        runtime.entities.get_mut(EntityIdx(2)).unwrap().runtime.magic_point = 200;
        runtime.relic_inequality_death(EntityIdx(2), &mut updates);
        assert_eq!(runtime.entities.get(EntityIdx(0)).unwrap().runtime.magic_point, 100);
    }

    // 背刺遗物第一次受击保留潜行，第二次受击才解除；普通队伍仍为一次。
    #[test]
    fn assassinate_requires_two_actual_hits_only_for_owner_team() {
        let mut runtime = fixture();
        runtime.assassinate_relic_team = Some(0);
        runtime.entities.get_mut(EntityIdx(0)).unwrap().runtime.assassinate = Some(AssassinateRuntime {
            fixed_lane: 0,
            target: EntityIdx(1),
            break_on_damage: true,
        });
        let mut updates = RunUpdates::new();
        runtime.run_plain_assassinate_post_damage_into(EntityIdx(0), 0, &mut updates);
        assert!(runtime.relic_assassinate_hits.is_empty());
        runtime.run_plain_assassinate_post_damage_into(EntityIdx(0), 1, &mut updates);
        assert!(runtime.entities.get(EntityIdx(0)).unwrap().runtime.assassinate.is_some());
        runtime.run_plain_assassinate_post_damage_into(EntityIdx(0), 1, &mut updates);
        assert!(runtime.entities.get(EntityIdx(0)).unwrap().runtime.assassinate.is_none());
        runtime.entities.get_mut(EntityIdx(1)).unwrap().runtime.assassinate = Some(AssassinateRuntime {
            fixed_lane: 0,
            target: EntityIdx(0),
            break_on_damage: true,
        });
        runtime.run_plain_assassinate_post_damage_into(EntityIdx(1), 1, &mut updates);
        assert!(runtime.entities.get(EntityIdx(1)).unwrap().runtime.assassinate.is_none());
    }
}
