//! 混沌爬塔结构化操作：同一份参数决定执行与卡面，独立于原技能概率与被动钩子。
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChaosOperation {
    pub op: String,
    pub target: String,
    pub value: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChaosCard {
    pub version: u32,
    pub name: String,
    pub ops: Vec<ChaosOperation>,
}
impl ChaosCard {
    /// 校验完整程序后再排入会话，未知操作不允许静默退化。
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > 8192 {
            return Err("chaos program too large".into());
        }
        let card: Self = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        if card.version != 1 || card.name.is_empty() || card.name.len() > 160 || card.ops.is_empty() || card.ops.len() > 4 {
            return Err("invalid chaos card".into());
        }
        for op in &card.ops {
            if !(1..=10000).contains(&op.value)
                || !matches!(
                    op.target.as_str(),
                    "enemy" | "enemies" | "self" | "ally" | "allies" | "dead_ally"
                )
                || !matches!(
                    op.op.as_str(),
                    "physical"
                        | "magic"
                        | "pure"
                        | "heal"
                        | "shield"
                        | "mp"
                        | "drain_mp"
                        | "haste"
                        | "slow"
                        | "ice"
                        | "poison"
                        | "burn"
                        | "curse"
                        | "berserk"
                        | "charm"
                        | "cleanse"
                        | "dispel"
                        | "revive"
                        | "atk_up"
                        | "def_up"
                        | "mag_up"
                        | "res_up"
                        | "agi_up"
                        | "wis_up"
                        | "speed_up"
                        | "iron"
                        | "max_hp"
                        | "sacrifice"
                        | "execute"
                        | "covid"
                        | "lazy"
                        | "pneumonia"
                        | "saitama"
                        | "summon"
                        | "shadow"
                        | "zombie"
                        | "clone"
                        | "merge"
                )
            {
                return Err("unknown chaos operation/target/value".into());
            }
        }
        Ok(card)
    }
}
impl CombatRuntime {
    fn chaos_targets(&mut self, actor: EntityIdx, selector: &str) -> Vec<EntityIdx> {
        let team = self.plain_effective_team(actor);
        if selector == "self" {
            return vec![actor];
        }
        let mut candidates: Vec<_> = self
            .entities
            .iter()
            .filter(|(id, e)| {
                let ally = e.runtime.team == team;
                match selector {
                    "enemy" | "enemies" => !ally && e.runtime.alive && e.runtime.hp > 0,
                    "ally" | "allies" => ally && e.runtime.alive && e.runtime.hp > 0,
                    "dead_ally" => {
                        ally && !e.runtime.alive && *id == e.runtime.root_owner && e.runtime.corpse != RuntimeCorpseKind::Merge
                    }
                    _ => false,
                }
            })
            .map(|(id, _)| id)
            .collect();
        if matches!(selector, "enemy" | "ally" | "dead_ally") && !candidates.is_empty() {
            let i = self.rng.next_i32(candidates.len() as i32) as usize;
            return vec![candidates.swap_remove(i)];
        }
        candidates
    }
    /// 每项操作重新选取合法目标；死亡立即结算，后续操作不命中已移除角色。
    pub fn drain_chaos_card_into(&mut self, actor: EntityIdx, card: ChaosCard, updates: &mut RunUpdates) {
        updates.add(RuntimeFrame::replay_update(
            actor.0 as usize,
            actor.0 as usize,
            format!("[0]使用[{}]", card.name),
            0,
        ));
        for operation in card.ops {
            if !self.entities.get(actor).is_some_and(|e| e.runtime.alive && e.runtime.hp > 0) {
                break;
            }
            let targets = self.chaos_targets(actor, &operation.target);
            for target in targets {
                let v = operation.value;
                match operation.op.as_str() {
                    "summon" | "shadow" | "zombie" | "clone" => {
                        // 复用原生召唤提交链；混沌子体不携带本体随机程序，避免无限递归。
                        let owner = self.entities.get(actor).unwrap();
                        let kind_export = match operation.op.as_str() {
                            "shadow" => profile::DEFAULT_CORE_SHADOW_KIND_EXPORT,
                            "zombie" => profile::DEFAULT_CORE_ZOMBIE_KIND_EXPORT,
                            "clone" => profile::DEFAULT_CORE_CLONE_KIND_EXPORT,
                            _ => profile::DEFAULT_CORE_SUMMON_KIND_EXPORT,
                        };
                        let kind = self
                            .registry
                            .player_kind_id_by_export_name(kind_export)
                            .expect("native chaos minion kind exists");
                        let hp = ((owner.template.max_hp as i64 * v as i64) / 100).clamp(1, i32::MAX as i64) as i32;
                        let child = PlayerTemplate::with_kind(
                            self.entities.len() + 1,
                            format!("{}?chaos{}", owner.template.name, self.entities.len()),
                            kind,
                            owner.runtime.team,
                            hp,
                            (owner.runtime.attack * v / 100).max(0),
                        )
                        // 机制类别与外显名称一致，战报直接使用原生召唤物名。
                        .with_display_name(match operation.op.as_str() {
                            "shadow" => "幻影",
                            "zombie" => "丧尸",
                            "clone" => "分身",
                            _ => "使魔",
                        })
                        .with_magic((owner.runtime.magic * v / 100).max(0))
                        .with_wisdom((owner.runtime.wisdom * v / 100).max(0))
                        .with_speed(owner.runtime.speed.max(1))
                        .with_def_res(
                            (owner.runtime.defense * v / 100).max(0),
                            (owner.runtime.resistance * v / 100).max(0),
                        )
                        .with_agility((owner.runtime.agility * v / 100).max(0));
                        let count = self
                            .entities
                            .iter()
                            .filter(|(id, e)| {
                                e.runtime.root_owner == owner.runtime.root_owner && *id != e.runtime.root_owner && e.runtime.alive
                            })
                            .count();
                        if count < 8 {
                            self.effects.push(QueuedEffect::SpawnWithMessage {
                                caster: actor,
                                template: child,
                                message: crate::runtime::lang::get_lang("sklSummoned").into(),
                            });
                            self.drain_effects_into(updates);
                        }
                    }
                    "merge" => {
                        if !self.entities.get(target).unwrap().runtime.alive {
                            self.apply_plain_merge_into(actor, target, updates);
                        }
                    }
                    "physical" | "magic" | "saitama" => {
                        let magic = operation.op == "magic";
                        let atp = self.entities.get(actor).unwrap().runtime.get_at(magic, &mut self.rng) * v as f64 / 100.0;
                        self.drain_plain_attack_with_atp_into(actor, target, magic, atp, updates);
                    }
                    "pure" | "execute" | "sacrifice" => {
                        let amount = if operation.op == "execute" {
                            (self.entities.get(target).unwrap().runtime.hp as i64 * v as i64 / 100).min(i32::MAX as i64) as i32
                        } else {
                            v
                        };
                        if self.apply_damage_into(actor, target, amount, updates) {
                            self.drain_lethal_damage_hooks_into(actor, target, updates);
                        }
                    }
                    "heal" => {
                        self.effects.push(QueuedEffect::Heal {
                            caster: actor,
                            target,
                            amount: v,
                        });
                        self.drain_effects_into(updates);
                    }
                    "revive" => {
                        if !self.entities.get(target).unwrap().runtime.alive {
                            self.effects.push(QueuedEffect::Revive {
                                caster: actor,
                                target,
                                hp: v,
                            });
                            self.drain_effects_into(updates);
                        }
                    }
                    "shield" => {
                        let e = self.entities.get_mut(target).unwrap();
                        e.runtime.shield = e.runtime.shield.saturating_add(v);
                    }
                    "mp" | "drain_mp" => {
                        let e = self.entities.get_mut(target).unwrap();
                        e.runtime.magic_point = if operation.op == "mp" {
                            (e.runtime.magic_point + v).min(100)
                        } else {
                            e.runtime.magic_point - v
                        };
                    }
                    "ice" => {
                        for _ in 0..v.min(4) {
                            self.apply_ice_on_damage(actor, target, updates);
                        }
                    }
                    "poison" => {
                        self.apply_poison_on_damage(actor, target, v.max(5), updates);
                    }
                    "burn" => {
                        for _ in 0..v.min(4) {
                            self.apply_fire_on_damage(actor, target, profile::PLAIN_FIRE_STATE_KEY);
                        }
                    }
                    "curse" => self.apply_curse_on_damage(actor, target, 1, updates),
                    "berserk" => {
                        self.apply_berserk_on_damage(actor, target, 1, updates);
                    }
                    "haste" | "slow" | "iron" => {
                        let (export, key) = match operation.op.as_str() {
                            "haste" => (profile::DEFAULT_CORE_HASTE_STATE_EXPORT, profile::PLAIN_HASTE_STATE_KEY),
                            "slow" => (profile::DEFAULT_CORE_SLOW_STATE_EXPORT, 78),
                            _ => (profile::DEFAULT_CORE_IRON_STATE_EXPORT, profile::PLAIN_IRON_STATE_KEY),
                        };
                        let state_id = self.registry.state_id_by_export_name(export).expect("core chaos state exists");
                        let priority = self.registry.state(state_id).unwrap().priority;
                        let turns = v.min(4);
                        let entry = match operation.op.as_str() {
                            "haste" => StateEntry::haste(key, state_id, 2, turns, priority),
                            "slow" => StateEntry::slow(key, state_id, turns, priority),
                            _ => StateEntry::iron(key, state_id, v, 2, priority),
                        };
                        let e = self.entities.get_mut(target).unwrap();
                        if !e.states.set_payload(key, entry.payload.clone()) {
                            e.states.add_entry(entry);
                        }
                        e.refresh_runtime_stats_from_template();
                    }
                    "charm" => {
                        self.drain_plain_charm_skill_into(actor, target, updates);
                    }
                    "cleanse" => {
                        let keys: Vec<_> = self
                            .entities
                            .get(target)
                            .unwrap()
                            .states
                            .entries()
                            .iter()
                            .filter(|e| {
                                matches!(
                                    e.payload,
                                    StatePayload::Ice { .. }
                                        | StatePayload::Poison { .. }
                                        | StatePayload::Curse { .. }
                                        | StatePayload::Slow { .. }
                                        | StatePayload::Berserk { .. }
                                        | StatePayload::Charm { .. }
                                        | StatePayload::FireMagHalfSteps(_)
                                )
                            })
                            .map(|e| e.legacy_order_key)
                            .collect();
                        let e = self.entities.get_mut(target).unwrap();
                        for key in keys {
                            let message = e.states.entry(key).and_then(|entry| match entry.payload {
                                StatePayload::Ice { .. } => Some("sklIceEnd"),
                                StatePayload::Poison { .. } => Some("sklPoisonEnd"),
                                StatePayload::Curse { .. } => Some("sklCurseEnd"),
                                StatePayload::Slow { .. } => Some("sklSlowEnd"),
                                StatePayload::Berserk { .. } => Some("sklBerserkEnd"),
                                StatePayload::Charm { .. } => Some("sklCharmEnd"),
                                _ => None,
                            });
                            e.states.clear_legacy_key(key);
                            if let Some(key) = message {
                                updates.add(RuntimeFrame::replay_update(
                                    actor.0 as usize,
                                    target.0 as usize,
                                    crate::runtime::lang::get_lang(key),
                                    0,
                                ));
                            }
                        }
                        e.refresh_runtime_stats_from_template();
                    }
                    "dispel" => {
                        self.apply_disperse_hit_into(actor, target, updates);
                    }
                    "covid" => {
                        self.infect_with_covid_into(actor, target, v.min(15), updates);
                    }
                    "lazy" => {
                        self.infect_with_lazy_into(actor, target, updates);
                    }
                    "pneumonia" => {
                        self.drain_covid_pneumonia_into(target, actor, v.min(15), updates);
                    }
                    _ => {
                        let e = self.entities.get_mut(target).unwrap();
                        // 永久加成只在当前战斗保留，模板与热字段同步，防止状态刷新冲掉数值。
                        match operation.op.as_str() {
                            "atk_up" => {
                                e.template.attack += v;
                                e.runtime.attack += v;
                            }
                            "def_up" => {
                                e.template.defense += v;
                                e.runtime.defense += v;
                            }
                            "mag_up" => {
                                e.template.magic += v;
                                e.runtime.magic += v;
                            }
                            "res_up" => {
                                e.template.resistance += v;
                                e.runtime.resistance += v;
                            }
                            "agi_up" => {
                                e.template.agility += v;
                                e.runtime.agility += v;
                            }
                            "wis_up" => {
                                e.template.wisdom += v;
                                e.runtime.wisdom += v;
                            }
                            "speed_up" => {
                                e.template.speed += v;
                                e.runtime.speed += v;
                            }
                            "max_hp" => {
                                e.template.max_hp += v;
                                e.runtime.hp += v;
                            }
                            _ => unreachable!(),
                        }
                    }
                }
                if crate::runtime::forced_action::controlled_team().is_some() {
                    self.cap_card_attributes();
                }
                // 原生入口已生成伤害/回复/状态文本，仅为无文本操作拼接原技能词片段。
                let message = match operation.op.as_str() {
                    "shield" => Some("[1]的[护盾]上升"),
                    "mp" | "mag_up" => Some("[1]魔力上升"),
                    "drain_mp" => Some("[1]魔力减少"),
                    "haste" => Some(crate::runtime::lang::get_lang("sklHasteHit")),
                    "slow" => Some(crate::runtime::lang::get_lang("sklSlowHit")),
                    "iron" => Some(crate::runtime::lang::get_lang("sklIrond")),
                    "atk_up" => Some(crate::runtime::lang::get_lang("sklAccumulated")),
                    "def_up" => Some("[1]防御力上升"),
                    "res_up" => Some("[1]抗性上升"),
                    "agi_up" => Some("[1]敏捷上升"),
                    "wis_up" => Some("[1]智慧上升"),
                    "speed_up" => Some("[1]速度上升"),
                    "max_hp" => Some("[1]体力上升"),
                    _ => None,
                };
                if let Some(message) = message {
                    updates.add(RuntimeFrame::replay_update(actor.0 as usize, target.0 as usize, message, 0));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    // 程序校验不执行部分效果，纯参数错误必须在排入行动之前拒绝。
    fn invalid_operations_and_versions_are_rejected() {
        assert!(ChaosCard::parse(r#"{"version":2,"name":"x","ops":[{"op":"shield","target":"self","value":5}]}"#).is_err());
        assert!(ChaosCard::parse(r#"{"version":1,"name":"x","ops":[{"op":"unknown","target":"self","value":5}]}"#).is_err());
        let valid =
            ChaosCard::parse(r#"{"version":1,"name":"护盾","ops":[{"op":"shield","target":"self","value":47}]}"#).unwrap();
        let mut runtime = CombatRuntime::from_template(PreparedCombatTemplate::new(vec![
            PlayerTemplate::new(1, "player", 0, 100, 20),
            PlayerTemplate::new(2, "enemy", 1, 100, 20),
        ]));
        runtime.drain_chaos_card_into(EntityIdx(0), valid, &mut RunUpdates::new());
        assert_eq!(runtime.entities.get(EntityIdx(0)).unwrap().runtime.shield, 47);
        assert_eq!(runtime.entities.get(EntityIdx(1)).unwrap().runtime.shield, 0);
    }
}
