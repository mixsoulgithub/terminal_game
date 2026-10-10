// 角色(职业).数据来自语料 corpus::CHARACTERS;
// 能不能开局要看起始牌组和起始遗物是不是都已经实现了.
use crate::core::cards;
use crate::core::corpus;
use crate::core::relics;

pub fn all() -> &'static [corpus::CharacterInfo] {
    corpus::CHARACTERS
}

pub fn by_index(i: usize) -> &'static corpus::CharacterInfo {
    let n = all().len();
    &all()[i % n.max(1)]
}

pub fn find(id: &str) -> Option<&'static corpus::CharacterInfo> {
    all().iter().find(|c| c.id == id)
}

/// 起始牌组里本游戏还没实现的牌
pub fn missing_cards(c: &corpus::CharacterInfo) -> Vec<&'static str> {
    c.deck
        .iter()
        .filter(|(id, _)| cards::card_def(id).is_none())
        .map(|(id, _)| *id)
        .collect()
}

/// 起始遗物实现了没
/// fx 全零的起始遗物(cracked_core/pure_water)只是占位,机制(充能球/Miracle)
/// 没实现,不能算"已实现";否则补上起始牌后角色会被误判为可开局
pub fn has_starter_relic(c: &corpus::CharacterInfo) -> bool {
    relics::relic_def(c.relic).is_some_and(|r| r.fx != relics::RelicFx::ZERO)
}

/// 这个角色现在能不能开局
pub fn playable(c: &corpus::CharacterInfo) -> bool {
    missing_cards(c).is_empty() && has_starter_relic(c)
}

/// 还没实现的部分,给角色选择界面显示
pub fn blocked_reason(c: &corpus::CharacterInfo) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let missing = missing_cards(c);
    if !missing.is_empty() {
        parts.push(format!("cards: {}", missing.join(" ")));
    }
    if !has_starter_relic(c) {
        parts.push(format!("relic: {}", c.relic));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}
