// 金标准测试:fixture 里的数值是拿 bun 直接跑参考实现(refs/slay-the-cli)
// 在同一颗种子下跑出来的(tools/gen_golden.ts 生成),这里断言我们的引擎逐位一致.
//
// 覆盖:第一章地图布局、开局生成的遭遇名单、第一个怪房间的怪物血量、
// 第一场战斗的三张卡牌奖励、base-35 种子的写法、药水身份序列(奖励/商店两条路径).
use crate::core::enemies;
use crate::core::map::ActMap;
use crate::core::run::{Run, Screen};
use crate::rng::{seed_to_string, Rng, RngRegistry, RunStream};

/// 多颗种子的同一个位置(fixture 里 CASES 用的行)
pub struct Case {
    pub seed: u64,
    pub burning: (i32, i32, i32),
    pub monsters: &'static [(&'static str, i32)],
    pub gold: i32,
    pub potion: Option<&'static str>,
    pub cards: &'static [(&'static str, bool)],
}

include!("golden_fixture.rs");

/// 参考实现的 id 是大写,本作用小写;史莱姆的 S/M/L 本作写全了
fn lower(s: &str) -> String {
    match s {
        "SPIKE_SLIME_S" => "spike_slime_small".to_string(),
        "ACID_SLIME_S" => "acid_slime_small".to_string(),
        "SPIKE_SLIME_M" => "spike_slime_medium".to_string(),
        "ACID_SLIME_M" => "acid_slime_medium".to_string(),
        "SPIKE_SLIME_L" => "spike_slime_large".to_string(),
        "ACID_SLIME_L" => "acid_slime_large".to_string(),
        _ => s.to_lowercase(),
    }
}

/// 把"赢了之后停留几帧"一步走完,落到奖励界面
fn settle(r: &mut Run) {
    r.sync_combat();
    for _ in 0..=Run::VICTORY_HOLD {
        r.tick_win_hold();
    }
}

#[test]
fn seed_string_matches_reference() {
    assert_eq!(seed_to_string(SEED), SEED_STRING);
}

#[test]
fn act1_map_layout_matches_reference() {
    // 第一章地图的流是 seed + 1
    let mut rng = Rng::new(SEED + 1);
    let map = ActMap::generate(&mut rng);
    let ours = map.to_rows_string();
    let want = MAP;
    for (i, (a, b)) in ours.lines().zip(want.lines()).enumerate() {
        assert_eq!(a.trim_end(), b.trim_end(), "第 {i} 行对不上");
    }
    assert_eq!(ours.lines().count(), want.lines().count(), "行数对不上");
}

/// 燃烧精英的选取与增益:两掷(mapRng)的结果要与参考实现逐位一致.
/// 参考实现里第一章一定会挑一个精英标记为燃烧,并掷一个 0..=3 的增益.
#[test]
fn burning_elite_matches_reference() {
    let mut rng = Rng::new(SEED + 1);
    let map = ActMap::generate(&mut rng);
    let (x, y, buff) = BURNING_ELITE;
    let idx = map.burning_node().expect("第一章应该有燃烧精英");
    let node = map.node(idx);
    assert_eq!(node.kind, crate::core::map::NodeKind::Elite, "燃烧的必须是精英");
    assert_eq!((node.col as i32, node.floor as i32), (x, y), "燃烧精英位置对不上");
    assert_eq!(map.burning_buff, buff, "燃烧精英增益对不上");
    // 整张图只标记一个
    assert_eq!(map.nodes.iter().filter(|n| n.burning).count(), 1);
}

#[test]
fn encounter_lists_match_reference() {
    let mut reg = RngRegistry::new(SEED);
    let lists = enemies::generate_encounters(1, reg.run(RunStream::MonsterRng));
    // 双方的遭遇 id 取名不同(参考实现大写、本作带后缀),比阵容更实在
    let ours = |ids: &[&'static str]| -> Vec<Vec<String>> {
        ids.iter()
            .map(|id| {
                enemies::resolve(id)
                    .enemies
                    .iter()
                    .map(|m| m.to_string())
                    .collect()
            })
            .collect()
    };
    let want = |lineups: &[&[&str]]| -> Vec<Vec<String>> {
        lineups
            .iter()
            .map(|l| l.iter().map(|m| lower(m)).collect())
            .collect()
    };
    assert_eq!(ours(&lists.monster), want(MONSTER_LINEUPS), "怪名单对不上");
    assert_eq!(ours(&lists.elite), want(ELITE_LINEUPS), "精英名单对不上");
    assert_eq!(ours(&lists.boss), want(BOSS_LINEUPS), "Boss 顺序对不上");
    assert_eq!(lists.monster.len(), MONSTER_LIST.len());
    assert_eq!(lists.elite.len(), ELITE_LIST.len());
    assert_eq!(lists.boss.len(), BOSS_ORDER.len());
    // 第一个怪房间消耗的是名单的头一条
    assert_eq!(FIRST_ROOM_ENCOUNTER, MONSTER_LIST[0]);
}

/// 第一层第一个可进的节点(地图上第 0 行那一列)
fn first_room_index(run: &Run) -> usize {
    run.map
        .row(0)
        .iter()
        .copied()
        .find(|i| run.map.node(*i).col == FIRST_ROOM_X)
        .expect("第一行有这一列")
}

#[test]
fn first_monster_room_hp_matches_reference() {
    let mut run = Run::new(SEED);
    let idx = first_room_index(&run);
    run.enter_node(idx).expect("第一层的怪房间进得去");
    let c = run.combat().expect("进怪房间就该开打");
    let ours: Vec<(String, i32)> = c
        .enemies
        .iter()
        .map(|e| (e.def.id.to_string(), e.max_hp))
        .collect();
    let want: Vec<(String, i32)> = FIRST_ROOM_MONSTERS
        .iter()
        .map(|(id, hp)| (lower(id), *hp))
        .collect();
    assert_eq!(ours, want);
}

#[test]
fn first_card_reward_matches_reference() {
    let mut run = Run::new(SEED);
    let idx = first_room_index(&run);
    run.enter_node(idx).expect("第一层的怪房间进得去");
    let lineups = enemies::resolve(run.combat().map(|c| c.encounter_id).unwrap_or("")).enemies;
    let started: Vec<String> = lineups.iter().map(|m| m.to_string()).collect();
    let want: Vec<String> = FIRST_ROOM_MONSTERS.iter().map(|(id, _)| lower(id)).collect();
    assert_eq!(started, want, "第一场遭遇对不上");
    run.debug_win_battle();
    settle(&mut run);
    assert_eq!(run.screen, Screen::Reward);
    let reward = run.reward.as_ref().expect("赢了就该有奖励");
    assert_eq!(reward.gold, REWARD_GOLD, "金币对不上");
    assert_eq!(
        reward.potion.map(|p| p.id.to_string()),
        REWARD_POTION.map(|s| s.to_string()),
        "药水对不上"
    );
    let ours: Vec<(String, bool)> = reward
        .cards
        .iter()
        .map(|c| (c.def.id.to_string(), c.upgraded))
        .collect();
    let want: Vec<(String, bool)> = REWARD_CARDS
        .iter()
        .map(|(id, up)| (lower(id), *up))
        .collect();
    assert_eq!(ours, want, "三选一的牌对不上");
}

/// 卡池的顺序也要对得上:抽牌是按池子下标抽的
#[test]
fn class_card_pool_sizes_match_reference() {
    use crate::core::card::Rarity;
    use crate::core::cards;
    assert_eq!(cards::reward_pool(Rarity::Common).len(), 20);
    assert_eq!(cards::reward_pool(Rarity::Uncommon).len(), 36);
    assert_eq!(cards::reward_pool(Rarity::Rare).len(), 16);
    // basics 切片里那三张排在各自稀有度的最前
    assert_eq!(cards::reward_pool(Rarity::Common)[0].id, "body_slam");
    assert_eq!(cards::reward_pool(Rarity::Common)[1].id, "anger");
    assert_eq!(cards::reward_pool(Rarity::Uncommon)[0].id, "whirlwind");
}




/// 多颗种子:第一个怪房间的阵容与血量、第一场战斗的金币/药水/三张牌
#[test]
fn multi_seed_golden_cases_match_reference() {
    for case in CASES {
        let mut run = Run::new(case.seed);
        // 燃烧精英的选取也要逐颗种子对上
        let bidx = run.map.burning_node().expect("第一章应该有燃烧精英");
        let bnode = run.map.node(bidx);
        assert_eq!(
            (bnode.col as i32, bnode.floor as i32, run.map.burning_buff),
            case.burning,
            "seed {} 的燃烧精英对不上",
            case.seed
        );
        let idx = run.map.row(0)[0];
        run.enter_node(idx).unwrap_or_else(|e| panic!("seed {}: {e}", case.seed));
        let ours: Vec<(String, i32)> = run
            .combat()
            .expect("进怪房间就该开打")
            .enemies
            .iter()
            .map(|e| (e.def.id.to_string(), e.max_hp))
            .collect();
        let want: Vec<(String, i32)> = case
            .monsters
            .iter()
            .map(|(id, hp)| (lower(id), *hp))
            .collect();
        assert_eq!(ours, want, "seed {} 的怪物血量对不上", case.seed);

        run.debug_win_battle();
        settle(&mut run);
        let reward = run.reward.as_ref().expect("赢了就该有奖励");
        assert_eq!(reward.gold, case.gold, "seed {} 的金币对不上", case.seed);
        // 药水:池子顺序与稀有度分档对齐后,连抽到的是哪一瓶都要一致
        assert_eq!(
            reward.potion.map(|p| p.id.to_string()),
            case.potion.map(lower),
            "seed {} 的药水身份对不上",
            case.seed
        );
        let got: Vec<(String, bool)> = reward
            .cards
            .iter()
            .map(|c| (c.def.id.to_string(), c.upgraded))
            .collect();
        let want: Vec<(String, bool)> = case
            .cards
            .iter()
            .map(|(id, up)| (lower(id), *up))
            .collect();
        assert_eq!(got, want, "seed {} 的三选一对不上", case.seed);
    }
}

// ---- Neow:掷选项、消耗流、祝福效果 ----

/// 掷出来的四个选项要逐项一致(祝福 id 与代价 id)
#[test]
fn neow_options_match_reference() {
    let run = Run::new(SEED);
    let ours: Vec<(String, String)> = run
        .neow_option_list()
        .iter()
        .map(|o| (o.bonus.to_string(), o.drawback.to_string()))
        .collect();
    let want: Vec<(String, String)> = NEOW_OPTIONS
        .iter()
        .map(|(b, d)| (lower(b), lower(d)))
        .collect();
    assert_eq!(ours, want, "Neow 的四个选项对不上");
    // 掷完选项后 neowRng 的计数器(参考实现多掷了一次 random(0))
    let mut rng = Rng::new(SEED);
    crate::core::events::neow_options(&mut rng);
    assert_eq!(rng.state().counter, NEOW_RNG_COUNTER);
}

/// 领第 2 项祝福后的生命/金币/牌组与参考一致
#[test]
fn neow_pick_matches_reference() {
    let mut run = Run::new(SEED);
    run.open_neow();
    // 参考实现里第 2 项就是这一档(无代价);选项列表已由 neow_options_match_reference 比对过
    let opt = run.neow_option_list()[1];
    assert_eq!(opt.bonus, lower(NEOW_PICK_BONUS));
    assert_eq!(opt.drawback, lower(NEOW_PICK_DRAWBACK));
    run.choose_event(1).expect("第 2 项祝福选得中");
    assert_eq!(run.player.hp, NEOW_PICK_HP, "生命对不上");
    assert_eq!(run.player.max_hp, NEOW_PICK_MAX_HP, "生命上限对不上");
    assert_eq!(run.player.gold, NEOW_PICK_GOLD, "金币对不上");
    assert_eq!(run.player.deck.len(), NEOW_PICK_DECK, "牌组张数对不上");
    run.leave_event();
    assert_eq!(run.screen, Screen::Map);
}

// ---- 未知房判定与事件池 ----

/// 连续判定 12 次未知房,结果序列与参考一致(一次 eventRng float + 递增概率)
#[test]
fn unknown_room_sequence_matches_reference() {
    let mut run = Run::new(SEED);
    let ours: Vec<String> = run
        .debug_unknown_rooms(UNKNOWN_ROOMS.len())
        .into_iter()
        .map(|s| s.to_lowercase())
        .collect();
    let want: Vec<String> = UNKNOWN_ROOMS.iter().map(|s| s.to_string()).collect();
    assert_eq!(ours, want, "未知房判定序列对不上");
}

/// 连续抽 12 次事件:按章事件池、0.25 神龛掷点、一次性事件抽走即移除
#[test]
fn event_picks_match_reference() {
    let mut run = Run::new(SEED);
    let ours: Vec<Option<String>> = run
        .debug_event_picks(EVENT_PICKS.len())
        .into_iter()
        .map(|o| o.map(lower))
        .collect();
    let want: Vec<Option<String>> = EVENT_PICKS.iter().map(|o| o.map(lower)).collect();
    assert_eq!(ours, want, "事件抽取序列对不上");
}

// ---- 商店货架 ----

/// 判定与抽取交替 8 轮:判定推进 eventRng,判成事件时才抽一个
/// (抽签掷在 eventRng 副本上,只有判定那一次 float 落在主流上)
#[test]
fn event_rolls_match_reference() {
    let mut run = Run::new(SEED);
    let mut ours: Vec<String> = Vec::new();
    for _ in 0..EVENT_ROLLS.len() {
        let kind = run.debug_unknown_rooms(1)[0].to_lowercase();
        // 只有判成事件才抽签:其它结果不该动事件池
        let id = if kind == "event" {
            run.debug_event_picks(1)[0]
        } else {
            None
        };
        match id {
            Some(id) => ours.push(format!("{kind}:{id}")),
            None => ours.push(kind),
        }
    }
    let want: Vec<String> = EVENT_ROLLS
        .iter()
        .map(|s| {
            s.split_once(':')
                .map(|(k, id)| format!("{k}:{}", lower(id)))
                .unwrap_or_else(|| (*s).to_string())
        })
        .collect();
    assert_eq!(ours, want, "判定与抽取交替的序列对不上");
}

/// 商店:牌的稀有度/价格/打折位、遗物档次与价格、药水价格、删牌价、流计数器
#[test]
fn shop_shelf_matches_reference() {
    let mut run = Run::new(SEED);
    run.open_shop();
    let shop = run.shop.as_ref().expect("进商店就该有货架");

    let cards: Vec<&crate::core::run::ShopItem> = shop.items.iter().collect();
    let our_cards: Vec<_> = cards
        .iter()
        .filter_map(|it| match it {
            crate::core::run::ShopItem::Card(c, price) => {
                let rarity = c.def.rarity.name().to_lowercase();
                Some((c.def.id.to_string(), rarity, *price))
            }
            _ => None,
        })
        .collect();
    let want_cards: Vec<(String, String, i32)> = SHOP_CARDS
        .iter()
        .map(|(id, rarity, price, _)| (lower(id), rarity.to_string(), *price))
        .collect();
    assert_eq!(
        our_cards
            .iter()
            .map(|(_, r, p)| (r.clone(), *p))
            .collect::<Vec<_>>(),
        want_cards
            .iter()
            .map(|(_, r, p)| (r.clone(), *p))
            .collect::<Vec<_>>(),
        "商店卡的稀有度与价格对不上"
    );
    assert_eq!(
        our_cards.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>(),
        want_cards.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>(),
        "商店卡的身份对不上"
    );

    let our_relics: Vec<(String, String, i32)> = shop
        .items
        .iter()
        .filter_map(|it| match it {
            crate::core::run::ShopItem::Relic(d, price) => {
                Some((d.id.to_string(), d.tier.name().to_lowercase(), *price))
            }
            _ => None,
        })
        .collect();
    assert_eq!(our_relics.len(), SHOP_RELICS.len(), "商店遗物件数对不上");
    for (i, (id, tier, price)) in SHOP_RELICS.iter().enumerate() {
        let ours = &our_relics[i];
        assert_eq!(ours.2, *price, "第 {i} 件遗物的价格对不上");
        assert_eq!(&ours.1, tier, "第 {i} 件遗物的档次对不上");
        assert_eq!(ours.0, lower(id), "第 {i} 件遗物的身份对不上");
    }

    // 药水:身份、价格与流位置都要和参考对上(池子顺序、稀有度分档一致后才可能)
    let our_potions: Vec<(String, i32)> = shop
        .items
        .iter()
        .filter_map(|it| match it {
            crate::core::run::ShopItem::Potion(d, price) => Some((d.id.to_string(), *price)),
            _ => None,
        })
        .collect();
    let want_potions: Vec<(String, i32)> = SHOP_POTIONS
        .iter()
        .map(|(id, price)| (lower(id), *price))
        .collect();
    assert_eq!(our_potions, want_potions, "商店药水的身份与价格对不上");

    let removal = shop
        .items
        .iter()
        .find_map(|it| match it {
            crate::core::run::ShopItem::Remove(price) => Some(*price),
            _ => None,
        })
        .expect("商店有删牌服务");
    assert_eq!(removal, SHOP_REMOVAL, "删牌服务价格对不上");

    let c = run.streams.run(crate::rng::RunStream::CardRng).state().counter;
    let m = run.streams.run(crate::rng::RunStream::MerchantRng).state().counter;
    let p = run.streams.run(crate::rng::RunStream::PotionRng).state().counter;
    assert_eq!(
        (c, m),
        (SHOP_STREAM_COUNTERS.0, SHOP_STREAM_COUNTERS.1),
        "商店的 cardRng/merchantRng 位置对不上"
    );
    assert_eq!(p, SHOP_STREAM_COUNTERS.2, "商店的药水流位置对不上");
}

// ---- 药水身份序列 ----

/// 奖励路径:连续 12 次掉落判定 + 稀有度掷点 + 池内抽签,身份与步数都要一致
#[test]
fn potion_reward_sequence_matches_reference() {
    let mut run = Run::new(SEED);
    let ours: Vec<Option<String>> = run
        .debug_potion_rewards(POTION_REWARD_SEQ.len())
        .into_iter()
        .map(|o| o.map(lower))
        .collect();
    let want: Vec<Option<String>> = POTION_REWARD_SEQ
        .iter()
        .map(|o| o.map(lower))
        .collect();
    assert_eq!(ours, want, "药水掉落的身份序列对不上");
    let (pity, counter) = run.debug_potion_state();
    assert_eq!(counter, POTION_REWARD_COUNTER, "药水流的步数对不上");
    assert_eq!(pity, POTION_REWARD_PITY, "药水保底值对不上");
}

/// 商店路径:连续 12 次(四家店 × 三瓶)只走稀有度掷点与池内抽签
#[test]
fn potion_shop_sequence_matches_reference() {
    let mut run = Run::new(SEED);
    let ours: Vec<String> = run
        .debug_potion_draws(POTION_SHOP_SEQ.len())
        .into_iter()
        .map(lower)
        .collect();
    let want: Vec<String> = POTION_SHOP_SEQ.iter().map(|s| lower(s)).collect();
    assert_eq!(ours, want, "商店药水的身份序列对不上");
    let (_, counter) = run.debug_potion_state();
    assert_eq!(counter, POTION_SHOP_COUNTER, "商店药水流的位置对不上");
}


// ---- 遗物掉落身份 ----

/// 战斗/事件档:连掷 12 次(档次走 relicRng,身份是开局洗好的池子里的下一件)
#[test]
fn relic_combat_drops_match_reference() {
    let mut run = Run::new(SEED);
    let ours = run.debug_relic_drops(RELIC_COMBAT_SEQ.len(), false);
    let want: Vec<(String, String)> = RELIC_COMBAT_SEQ
        .iter()
        .map(|(tier, id)| (tier.to_string(), lower(id)))
        .collect();
    assert_eq!(ours, want, "战斗/事件档的遗物身份对不上");
}

/// 精英档:连掷 12 次
#[test]
fn relic_elite_drops_match_reference() {
    let mut run = Run::new(SEED);
    let ours = run.debug_relic_drops(RELIC_ELITE_SEQ.len(), true);
    let want: Vec<(String, String)> = RELIC_ELITE_SEQ
        .iter()
        .map(|(tier, id)| (tier.to_string(), lower(id)))
        .collect();
    assert_eq!(ours, want, "精英档的遗物身份对不上");
}

/// Boss 三选一:四组,每组三件(不掷点,直接按池子顺序取)
#[test]
fn relic_boss_choices_match_reference() {
    let mut run = Run::new(SEED);
    let ours = run.debug_relic_boss_choices(RELIC_BOSS_CHOICES.len());
    let want: Vec<Vec<String>> = RELIC_BOSS_CHOICES
        .iter()
        .map(|set| set.iter().map(|id| lower(id)).collect())
        .collect();
    assert_eq!(ours, want, "Boss 遗物三选一对不上");
}

/// 宝箱:连开 6 个,尺寸/金币/档次/身份都要一致(尺寸与金币那两掷也要对得上)
#[test]
fn relic_chests_match_reference() {
    let mut run = Run::new(SEED);
    let ours = run.debug_relic_chests(RELIC_CHESTS.len());
    let want: Vec<(String, bool, String, String)> = RELIC_CHESTS
        .iter()
        .map(|(size, gold, tier, id)| (size.to_string(), *gold, tier.to_string(), lower(id)))
        .collect();
    assert_eq!(ours, want, "宝箱的遗物身份对不上");
}
