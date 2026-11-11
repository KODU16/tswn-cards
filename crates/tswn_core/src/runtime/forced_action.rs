//! cards 前端的强制行动状态；每场 BattleSession 都保有自己的副本。

use std::cell::RefCell;

/// 前端封包解码后的行动和药水；先校验完整输入，避免残缺指令污染下一局。
pub struct DecodedActions {
    pub pairs: Vec<(u32, u32, Option<u32>)>,
    pub potions: Vec<(u32, u32)>,
}

/// 每场对局独有的 cards 控制状态。
#[derive(Debug, Default, Clone)]
pub struct ForcedState {
    pub queue: Vec<(u32, u32, Option<u32>)>,
    pub potions: Vec<(u32, u32)>,
    pub active_potions: Vec<(u32, u8, i32)>,
    pub controlled_team: Option<usize>,
    pub paused_actor: Option<u32>,
    pub fallback: Option<(u32, String)>,
    pub hits: u32,
    pub taken: u32,
}

thread_local! {
    // 批量回放先写入该暂存槽，BattleDriver 创建时接管；运行时只通过 with_state 访问会话副本。
    static CURRENT: RefCell<ForcedState> = RefCell::new(ForcedState::default());
}

/// 在推进一个 Runtime 回合时临时绑定该会话状态，防止交错会话相互串指令。
pub fn with_state<T>(state: &mut ForcedState, f: impl FnOnce() -> T) -> T {
    CURRENT.with(|current| std::mem::swap(&mut *current.borrow_mut(), state));
    let result = f();
    CURRENT.with(|current| std::mem::swap(&mut *current.borrow_mut(), state));
    result
}

/// 将批量回放预先登记的状态交给刚创建的 BattleDriver。
pub fn take_pending_actions() -> ForcedState { CURRENT.with(|current| std::mem::take(&mut *current.borrow_mut())) }

/// 校验 cards 的 actor/skill/packed 三元组协议。
pub fn decode_actions(actions: &[u32]) -> Result<DecodedActions, String> {
    if actions.len() % 3 != 0 {
        return Err("actions must contain complete actor/skill/packed triples".into());
    }
    let mut pairs = Vec::with_capacity(actions.len() / 3);
    let mut potions = Vec::new();
    for chunk in actions.chunks_exact(3) {
        let (actor, skill, packed) = (chunk[0], chunk[1], chunk[2]);
        if skill > 24 && skill != 255 {
            return Err("skill id must be 0..=24 or 255".into());
        }
        if packed > 0xffff {
            return Err("packed action exceeds the 16-bit cards protocol".into());
        }
        let potion = packed >> 8;
        if potion != 0 && !(1..=7).contains(&potion) && potion != 9 && potion != 10 {
            return Err("unknown potion code".into());
        }
        let mp = packed & 0xff;
        pairs.push((actor, skill, (mp > 0).then_some(mp)));
        if potion > 0 {
            potions.push((actor, potion));
        }
    }
    Ok(DecodedActions { pairs, potions })
}

pub fn set(pairs: Vec<(u32, u32, Option<u32>)>) { CURRENT.with(|state| state.borrow_mut().queue = pairs); }
pub fn set_potions(potions: Vec<(u32, u32)>) { CURRENT.with(|state| state.borrow_mut().potions = potions); }
pub fn clear() { CURRENT.with(|state| *state.borrow_mut() = ForcedState::default()); }
pub fn pending() -> usize { CURRENT.with(|state| state.borrow().queue.len()) }

pub fn take(actor: u32) -> Option<(u32, Option<u32>)> {
    CURRENT.with(|state| {
        let mut state = state.borrow_mut();
        let index = state.queue.iter().position(|(id, _, _)| *id == actor)?;
        let (_, skill, mp) = state.queue.remove(index);
        state.taken += 1;
        Some((skill, mp))
    })
}

pub fn hit() { CURRENT.with(|state| state.borrow_mut().hits += 1); }
pub fn hits() -> u32 { CURRENT.with(|state| state.borrow().hits) }
pub fn taken() -> u32 { CURRENT.with(|state| state.borrow().taken) }
pub fn controlled_team() -> Option<usize> { CURRENT.with(|state| state.borrow().controlled_team) }
pub fn set_pause(actor: u32) { CURRENT.with(|state| state.borrow_mut().paused_actor = Some(actor)); }
pub fn take_pause() -> Option<u32> { CURRENT.with(|state| state.borrow_mut().paused_actor.take()) }
pub fn record_fallback(actor: u32, reason: impl Into<String>) {
    CURRENT.with(|state| state.borrow_mut().fallback = Some((actor, reason.into())));
}
pub fn take_fallback() -> Option<(u32, String)> { CURRENT.with(|state| state.borrow_mut().fallback.take()) }

pub fn take_potion(actor: u32) -> Option<u32> {
    CURRENT.with(|state| {
        let mut state = state.borrow_mut();
        let index = state.potions.iter().position(|(id, _)| *id == actor)?;
        Some(state.potions.remove(index).1)
    })
}
pub fn set_active_potion(value: Option<(u32, u8, i32)>) {
    if let Some(value) = value {
        CURRENT.with(|state| state.borrow_mut().active_potions.push(value));
    }
}
pub fn take_active_potion(actor: u32) -> Option<(u32, u8, i32)> {
    CURRENT.with(|state| {
        let mut state = state.borrow_mut();
        let index = state.active_potions.iter().position(|(id, _, _)| *id == actor)?;
        Some(state.active_potions.remove(index))
    })
}
