// 状态(增益/减益/能力).玩家与敌人共用同一套结构,右侧数值表示层数或强度.
// 减益(is_debuff)在持有者回合结束时减一;能力(is_power)永久保留.
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Status {
    // 通用增减益
    Strength,
    Dexterity,
    Vulnerable,
    Weak,
    Frail,
    Entangled,
    // 玩家能力
    DemonForm,
    Metallicize,
    FeelNoPain,
    DarkEmbrace,
    Evolve,
    FireBreathing,
    Barricade,
    Brutality,
    Rupture,
    Regenerate,
    Thorns,
    Berserk,
    Combust,
    Corruption,
    DoubleTap,
    Juggernaut,
    Rage,
    FlameBarrier,
    Artifact,
    /// 本回合起一段时间内不能从卡牌获得格挡(紧急按钮)
    NoBlock,
    /// 本回合不能再抽牌(战斗恍惚)
    NoDraw,
    Mayhem,
    Magnetism,
    Panache,
    SadisticNature,
    // 玩家与敌人共用、来自敌人的状态
    /// 回合结束获得等量格挡,受到未被格挡的伤害时减一
    PlatedArmor,
    /// 玩家回合结束时受到等量伤害(缠绕)
    Constricted,
    /// 抽到的牌费用随机化(蛇眼)
    Confused,
    /// 每打出一张非攻击牌就往抽牌堆洗入等量眩晕(被选中者)
    Hex,
    /// 下回合少抽等量张牌
    DrawReduction,
    /// 被两面夹击:从背后受到攻击时多吃 50% 伤害
    Surrounded,
    /// 受到的所有伤害降为 1(复仇女神的循环)
    Intangible,
    // 敌人能力
    Ritual,
    Enrage,
    /// 受到攻击伤害时获得等量力量(狂怒小鬼)
    Anger,
    /// 玩家每打出一张攻击牌就受到等量伤害(守护者的防御姿态)
    SharpHide,
    /// 玩家每打出一张牌就受到等量伤害(心脏)
    BeatOfDeath,
    /// 玩家每打出一张能力牌就获得等量力量(觉醒者)
    Curiosity,
    /// 第一次受到攻击伤害时获得等量格挡,一次性(虱子)
    CurlUp,
    /// 倒计时,回合数到就自爆
    Explosive,
    /// 倒计时,回合数到就消失(瞬变体)
    Fading,
    /// 受到的攻击伤害减半,每次被命中掉一层,掉光则落地眩晕(鸟)
    Flight,
    /// 本回合最多再掉等量生命(心脏)
    Invincible,
    /// 受到攻击伤害时获得等量格挡,每命中一次层数加一,自己回合结束重置(扭动巨物/蛇草)
    Malleable,
    /// 首领死亡时一起退场(召唤物)
    Minion,
    /// 死亡时带走所有召唤物
    MinionLeader,
    /// 掉够等量生命就换防御姿态(守护者)
    ModeShift,
    /// 造成未被格挡的攻击伤害时往玩家弃牌堆塞等量伤口(痛苦刺击)
    PainfulStabs,
    /// 受到攻击伤害就重新选招(扭动巨物).
    /// 仅作展示:重掷走 EnemyDef::special = Special::Reactive(combat.rs hit_enemy 那段),
    /// 引擎不读这个状态本身.
    Reactive,
    /// 死亡后若还有同伴就半血复活一次(暗灵)
    Regrow,
    /// 掉血时临时扣除等量力量,自己回合结束回补(瞬变体)
    Shifting,
    /// 玩家每打出一张牌就多吃 10% 攻击伤害,自己回合结束重置(巨大头颅)
    Slow,
    /// 死亡时给玩家上等量易伤(孢子云)
    SporeCloud,
    /// 血量掉到一半就分裂成两只小史莱姆.
    /// 仅作展示:真正的分裂走 EnemyDef::special = Special::Split(combat.rs on_enemy_hp_lost),
    /// 引擎不读这个状态本身.
    Split,
    /// 身上压着一张被偷走的牌,死亡时归还.
    /// 仅作展示:被偷的牌记在 Combat::stasis 里,归还走 return_stolen_card,引擎不读这个状态.
    Stasis,
    /// 每回合结束时获得等量力量(圆球步行者)
    StrengthUp,
    /// 玩家每打出第 12 张牌就结束其回合并获得等量力量(时间吞噬者)
    TimeWarp,
    /// 睡着(拉格文):不行动,挨打会醒
    Asleep,
    // 药水需要的状态
    /// 接下来的 n 张牌打两次(复制药水)
    Duplication,
    /// 自己回合结束时扣掉等量力量(力量药水)
    LoseStrength,
    /// 自己回合结束时扣掉等量敏捷(敏捷药水)
    LoseDexterity,
    /// 集中:本作没有充能球,挂上也不影响任何计算(集中药水).
    /// 超出范围:集中药水是蓝职(Defect)专属(class="blue"),本作只有铁甲能开局,
    /// 该药水抽不到,所以这个状态不会被挂上、引擎也不读.
    Focus,
    /// 自己回合开始时掉等量生命再减一层(毒药水)
    Poison,
    // 绿职(Silent)的能力与敌人身上的标记
    /// 飞刀(Shiv)多打等量伤害(精准)
    Accuracy,
    /// 每打出一张牌获得等量格挡(残影)
    AfterImage,
    /// 每打出一张牌对所有敌人造成等量伤害(千刀万剐)
    ThousandCuts,
    /// 攻击造成未被格挡的伤害时给目标挂等量中毒(涂毒)
    Envenom,
    /// 自己回合开始往手里塞一张飞刀(无尽苦痛之刃);层数只作展示
    InfiniteBlades,
    /// 自己回合开始对所有敌人挂等量中毒(毒雾)
    NoxiousFumes,
    /// 自己回合开始抽 1 弃 1(千机万巧)
    ToolsOfTheTrade,
    /// 回合结束时掉 1 敏捷(幽魂形态);层数只作展示
    WraithForm,
    /// 下个回合开始不清空格挡(残影术);层数 = 还剩几个回合
    Blur,
    /// 下个回合攻击伤害翻倍(幻影杀手);层数 = 还剩几个回合
    PhantasmalKiller,
    /// 接下来等量张技能牌打两次(爆发)
    Burst,
    /// 回合结束时最多保留等量张手牌(周密计划)
    WellLaidPlans,
    /// 本回合每打出一张牌就掉等量生命(窒息)
    Choke,
    /// 死亡时按最大生命对所有敌人造成伤害(尸爆术)
    CorpseExplosion,
    /// 本回合攻击伤害翻倍(幻影杀手).回合末随减益一起清掉
    DoubleDamage,
}

impl Status {
    /// 图鉴"buff 汇总"要一条不漏:这里是全部状态变体.
    /// 新增变体时 status_all_lists_every_variant 会失败(它去数 name() 的分支数).
    pub const ALL: &'static [Status] = &[
        Status::Strength, Status::Dexterity, Status::Vulnerable, Status::Weak, Status::Frail, Status::Entangled,
        Status::DemonForm, Status::Metallicize, Status::FeelNoPain, Status::DarkEmbrace, Status::Evolve, Status::FireBreathing,
        Status::Barricade, Status::Brutality, Status::Rupture, Status::Regenerate, Status::Thorns, Status::Berserk,
        Status::Combust, Status::Corruption, Status::DoubleTap, Status::Juggernaut, Status::Rage, Status::FlameBarrier,
        Status::Artifact, Status::NoBlock, Status::NoDraw, Status::Mayhem, Status::Magnetism, Status::Panache,
        Status::SadisticNature, Status::PlatedArmor, Status::Constricted, Status::Confused, Status::Hex, Status::DrawReduction,
        Status::Surrounded, Status::Intangible, Status::Ritual, Status::Enrage, Status::Anger, Status::SharpHide,
        Status::BeatOfDeath, Status::Curiosity, Status::CurlUp, Status::Explosive, Status::Fading, Status::Flight,
        Status::Invincible, Status::Malleable, Status::Minion, Status::MinionLeader, Status::ModeShift, Status::PainfulStabs,
        Status::Reactive, Status::Regrow, Status::Shifting, Status::Slow, Status::SporeCloud, Status::Split,
        Status::Stasis, Status::StrengthUp, Status::TimeWarp, Status::Asleep, Status::Duplication, Status::LoseStrength,
        Status::LoseDexterity, Status::Focus, Status::Poison, Status::Accuracy, Status::AfterImage, Status::ThousandCuts,
        Status::Envenom, Status::InfiniteBlades, Status::NoxiousFumes, Status::ToolsOfTheTrade, Status::WraithForm, Status::Blur,
        Status::PhantasmalKiller, Status::Burst, Status::WellLaidPlans, Status::Choke, Status::CorpseExplosion, Status::DoubleDamage,
    ];






    /// 图鉴里每条状态前显示一个 Nerd Font 图标.图标按**字形名**从字体 cmap 里挑
    /// (映射表见 tools/nf_status_glyphs.py),源码里一律写 \u{...} 转义,文件保持 ASCII.
    /// 换图标只改那张表再重跑;先在 compendium 里看效果,确认后再往 HUD/战斗界面铺.
    pub fn glyph(self) -> char {
        use Status::*;
        match self {
            Strength => '\u{ed67}', // fa-dumbbell
            Dexterity => '\u{ee14}', // fa-shoe_prints
            Vulnerable => '\u{ef4f}', // fa-heart_crack
            Weak => '\u{f04e5}', // md-sword
            Frail => '\u{f099e}', // md-shield_off
            Entangled => '\u{ef10}', // fa-spider
            DemonForm => '\u{ef76}', // fa-fire_flame_curved
            Metallicize => '\u{ed25}', // fa-shield_halved
            FeelNoPain => '\u{ed74}', // fa-bandage
            DarkEmbrace => '\u{f186}', // fa-moon_o
            Evolve => '\u{f0638}', // md-cards
            FireBreathing => '\u{f06d}', // fa-fire
            Barricade => '\u{f07fe}', // md-wall
            Brutality => '\u{f0cec}', // md-blood_bag
            Rupture => '\u{ee8e}', // fa-droplet_slash
            Regenerate => '\u{f05f6}', // md-heart_pulse
            Thorns => '\u{f0db5}', // md-cactus
            Berserk => '\u{ee50}', // fa-face_grin_stars
            Combust => '\u{ed78}', // fa-fire_flame_simple
            Corruption => '\u{ee15}', // fa-skull
            DoubleTap => '\u{f0787}', // md-sword_cross
            Juggernaut => '\u{ef84}', // fa-helmet_safety
            Rage => '\u{ee1f}', // fa-face_angry
            FlameBarrier => '\u{e0c0}', // ple-flame_thick
            Artifact => '\u{f219}', // fa-gem
            NoBlock => '\u{f05e}', // fa-ban
            NoDraw => '\u{eabd}', // cod-circle_slash
            Mayhem => '\u{f074}', // fa-shuffle
            Magnetism => '\u{f076}', // fa-magnet
            Panache => '\u{edf7}', // fa-feather
            SadisticNature => '\u{ee53}', // fa-face_grin_tongue_squint
            PlatedArmor => '\u{f0ada}', // md-shield_plus
            Constricted => '\u{f150e}', // md-snake
            Confused => '\u{f02d7}', // md-help_circle
            Hex => '\u{ef21}', // fa-hurricane
            DrawReduction => '\u{f0639}', // md-cards_outline
            Surrounded => '\u{f0849}', // md-account_group
            Intangible => '\u{f02a0}', // md-ghost
            Ritual => '\u{eedb}', // fa-hands_praying
            Enrage => '\u{f0a1}', // fa-bullhorn
            Anger => '\u{ee48}', // fa-face_grimace
            SharpHide => '\u{eefd}', // fa-hand_fist
            BeatOfDeath => '\u{f21e}', // fa-heartbeat
            Curiosity => '\u{f0349}', // md-magnify
            CurlUp => '\u{f0adb}', // md-shield_plus_outline
            Explosive => '\u{f0691}', // md-bomb
            Fading => '\u{f253}', // fa-hourglass_end
            Flight => '\u{ed99}', // fa-dove
            Invincible => '\u{f132}', // fa-shield
            Malleable => '\u{eeb0}', // fa-shapes
            Minion => '\u{f0c0}', // fa-users
            MinionLeader => '\u{edeb}', // fa-crown
            ModeShift => '\u{f2f1}', // fa-rotate
            PainfulStabs => '\u{ed8d}', // fa-syringe
            Reactive => '\u{f0db3}', // md-bolt
            Regrow => '\u{eda7}', // fa-seedling
            Shifting => '\u{f07d}', // fa-arrows_v
            Slow => '\u{f017}', // fa-clock_o
            SporeCloud => '\u{f0c2}', // fa-cloud
            Split => '\u{f062c}', // md-source_branch
            Stasis => '\u{f187}', // fa-box_archive
            StrengthUp => '\u{f102}', // fa-angles_up
            TimeWarp => '\u{f254}', // fa-hourglass
            Asleep => '\u{f04b2}', // md-sleep
            Duplication => '\u{f24d}', // fa-clone
            LoseStrength => '\u{f063}', // fa-arrow_down
            LoseDexterity => '\u{ea9a}', // cod-arrow_down
            Focus => '\u{f140}', // fa-bullseye
            Poison => '\u{ef0e}', // fa-skull_crossbones
            Accuracy => '\u{f05b}', // fa-crosshairs
            AfterImage => '\u{f00f}', // fa-images
            ThousandCuts => '\u{f09fb}', // md-knife
            Envenom => '\u{f0c3}', // fa-flask
            InfiniteBlades => '\u{eecf}', // fa-khanda
            NoxiousFumes => '\u{ef29}', // fa-smog
            ToolsOfTheTrade => '\u{ef70}', // fa-screwdriver_wrench
            WraithForm => '\u{eefe}', // fa-ghost
            Blur => '\u{f070}', // fa-eye_slash
            PhantasmalKiller => '\u{f21b}', // fa-user_secret
            Burst => '\u{f04e}', // fa-forward
            WellLaidPlans => '\u{eab3}', // cod-checklist
            Choke => '\u{f0a4f}', // md-hand_front_right
            CorpseExplosion => '\u{f1e2}', // fa-bomb
            DoubleDamage => '\u{ee29}', // fa-check_double
        }
    }

    /// 图标配色键:键到颜色的映射在 theme::icon_color(先在 compendium 里审,
    /// 确认后再往 HUD/战斗状态行铺).
    pub fn icon_color_key(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "fire",
            Dexterity => "good",
            Vulnerable => "bad",
            Weak => "bad",
            Frail => "bad",
            Entangled => "bad",
            DemonForm => "fire",
            Metallicize => "block",
            FeelNoPain => "good",
            DarkEmbrace => "good",
            Evolve => "good",
            FireBreathing => "fire",
            Barricade => "block",
            Brutality => "blood",
            Rupture => "blood",
            Regenerate => "good",
            Thorns => "fire",
            Berserk => "fire",
            Combust => "fire",
            Corruption => "arcane",
            DoubleTap => "energy",
            Juggernaut => "fire",
            Rage => "block",
            FlameBarrier => "block",
            Artifact => "good",
            NoBlock => "bad",
            NoDraw => "energy",
            Mayhem => "energy",
            Magnetism => "good",
            Panache => "good",
            SadisticNature => "fire",
            PlatedArmor => "block",
            Constricted => "bad",
            Confused => "arcane",
            Hex => "arcane",
            DrawReduction => "energy",
            Surrounded => "arcane",
            Intangible => "block",
            Ritual => "arcane",
            Enrage => "arcane",
            Anger => "arcane",
            SharpHide => "block",
            BeatOfDeath => "blood",
            Curiosity => "arcane",
            CurlUp => "good",
            Explosive => "fire",
            Fading => "ghost",
            Flight => "block",
            Invincible => "block",
            Malleable => "block",
            Minion => "plain",
            MinionLeader => "plain",
            ModeShift => "arcane",
            PainfulStabs => "blood",
            Reactive => "ghost",
            Regrow => "good",
            Shifting => "arcane",
            Slow => "ghost",
            SporeCloud => "block",
            Split => "dim",
            Stasis => "dim",
            StrengthUp => "arcane",
            TimeWarp => "ghost",
            Asleep => "ghost",
            Duplication => "energy",
            LoseStrength => "blood",
            LoseDexterity => "blood",
            Focus => "good",
            Poison => "poison",
            Accuracy => "good",
            AfterImage => "good",
            ThousandCuts => "fire",
            Envenom => "poison",
            InfiniteBlades => "good",
            NoxiousFumes => "poison",
            ToolsOfTheTrade => "good",
            WraithForm => "ghost",
            Blur => "block",
            PhantasmalKiller => "fire",
            Burst => "energy",
            WellLaidPlans => "good",
            Choke => "poison",
            CorpseExplosion => "poison",
            DoubleDamage => "fire",
        }
    }

    /// 原作这条状态的图标长什么样(据资料与观察;带 ? 的不确定,评审时纠正)
    pub fn original_icon_note(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "屈臂肌肉",
            Dexterity => "带翼的靴子/脚步",
            Vulnerable => "裂开的心",
            Weak => "断剑",
            Frail => "破盾",
            Entangled => "蛛网",
            DemonForm => "恶魔头(带角)",
            Metallicize => "金属齿轮",
            FeelNoPain => "绷带",
            DarkEmbrace => "暗色斗篷?",
            Evolve => "卡牌+上箭头",
            FireBreathing => "龙头吐火",
            Barricade => "铁壁",
            Brutality => "血滴",
            Rupture => "血滴+上箭头",
            Regenerate => "心+加号",
            Thorns => "荆棘球/仙人球",
            Berserk => "红色怒面",
            Combust => "火焰",
            Corruption => "紫色骷髅",
            DoubleTap => "两把交叉的剑",
            Juggernaut => "骑士头盔",
            Rage => "怒面",
            FlameBarrier => "火焰盾",
            Artifact => "宝石/宝珠",
            NoBlock => "带斜杠的盾",
            NoDraw => "带斜杠的卡",
            Mayhem => "混乱漩涡",
            Magnetism => "磁铁",
            Panache => "羽饰帽",
            SadisticNature => "诡异的笑面",
            PlatedArmor => "板甲盾",
            Constricted => "缠绕的蛇/藤",
            Confused => "问号/漩涡",
            Hex => "紫色漩涡",
            DrawReduction => "卡牌+下箭头",
            Surrounded => "两侧的敌人/人群",
            Intangible => "幽魂",
            Ritual => "图腾/烛火?",
            Enrage => "怒号/喇叭",
            Anger => "怒面",
            SharpHide => "尖刺盾",
            BeatOfDeath => "心跳/心",
            Curiosity => "放大镜/问号?",
            CurlUp => "蜷缩的虫",
            Explosive => "炸弹",
            Fading => "沙漏",
            Flight => "翅膀",
            Invincible => "盾+锁",
            Malleable => "变形方块",
            Minion => "小怪剪影",
            MinionLeader => "王冠",
            ModeShift => "齿轮/盾",
            PainfulStabs => "针/匕首",
            Reactive => "闪电",
            Regrow => "嫩芽",
            Shifting => "双向箭头",
            Slow => "沙漏",
            SporeCloud => "孢子云",
            Split => "分裂的史莱姆",
            Stasis => "上锁的卡/盒子",
            StrengthUp => "上箭头",
            TimeWarp => "时钟/沙漏",
            Asleep => "沉睡(月亮/zzz)",
            Duplication => "两张卡",
            LoseStrength => "向下箭头",
            LoseDexterity => "向下箭头",
            Focus => "球/眼睛",
            Poison => "骷髅+毒",
            Accuracy => "靶心+飞刀",
            AfterImage => "残影/分身",
            ThousandCuts => "许多刀刃",
            Envenom => "毒牙/绿色毒滴?",
            InfiniteBlades => "无尽的剑",
            NoxiousFumes => "毒云",
            ToolsOfTheTrade => "工具",
            WraithForm => "幽魂",
            Blur => "残像/抹影?",
            PhantasmalKiller => "面具/幻影刺客",
            Burst => "爆发前冲",
            WellLaidPlans => "地图/计划",
            Choke => "掐住的手",
            CorpseExplosion => "爆炸的骷髅",
            DoubleDamage => "双重影像",
        }
    }

    /// 候选图标(最多 10 个,第一个是当前用的):图鉴把这份清单附在说明后面,
    /// 评审时按序号或字形名挑.候选也是按字形名从字体 cmap 里搜出来的,挑到的都在字体里.
    pub fn glyph_candidates(self) -> &'static [(char, &'static str)] {
        use Status::*;
        match self {
            Strength => &[('\u{ed67}', "fa-dumbbell"), ('\u{f01e6}', "md-dumbbell"), ('\u{ee94}', "fa-weight-hanging"), ('\u{ed93}', "fa-weight-scale"), ('\u{f05c9}', "md-format-line-weight"), ('\u{f05a1}', "md-weight"), ('\u{f0d3f}', "md-weight-gram"), ('\u{f05a2}', "md-weight-kilogram"), ('\u{f115d}', "md-weight-lifter"), ('\u{f09b5}', "md-weight-pound")],
            Dexterity => &[('\u{ee14}', "fa-shoe-prints"), ('\u{f0a58}', "md-horseshoe"), ('\u{f15ca}', "md-shoe-ballet"), ('\u{f15c7}', "md-shoe-cleat"), ('\u{f0b47}', "md-shoe-formal"), ('\u{f0b48}', "md-shoe-heel"), ('\u{f0dfa}', "md-shoe-print"), ('\u{f15c8}', "md-shoe-sneaker"), ('\u{f1a72}', "md-snowshoeing"), ('\u{eb7b}', "cod-vm-running")],
            Vulnerable => &[('\u{ef4f}', "fa-heart-crack"), ('\u{f02d4}', "md-heart-broken"), ('\u{f0d14}', "md-heart-broken-outline"), ('\u{ed25}', "fa-shield-halved"), ('\u{ef05}', "fa-house-chimney-crack"), ('\u{f02ed}', "md-image-broken"), ('\u{f02ee}', "md-image-broken-variant"), ('\u{f0da4}', "md-trophy-broken"), ('\u{ebf8}', "cod-target"), ('\u{f04fe}', "md-target")],
            Weak => &[('\u{f04e5}', "md-sword"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword"), ('\u{f18bf}', "md-shield-sword-outline"), ('\u{f0787}', "md-sword-cross"), ('\u{ec08}', "cod-bell-slash"), ('\u{ec09}', "cod-bell-slash-dot"), ('\u{eabd}', "cod-circle-slash"), ('\u{f1f6}', "fa-bell-slash")],
            Frail => &[('\u{f099e}', "md-shield-off"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield"), ('\u{ed25}', "fa-shield-halved"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline"), ('\u{f0498}', "md-shield"), ('\u{f088f}', "md-shield-account"), ('\u{f0a12}', "md-shield-account-outline")],
            Entangled => &[('\u{ef10}', "fa-spider"), ('\u{f11ea}', "md-spider"), ('\u{f11eb}', "md-spider-thread"), ('\u{f0bca}', "md-spider-web"), ('\u{e845}', "dev-nodewebkit"), ('\u{e8e1}', "dev-webflow"), ('\u{e8e2}', "dev-weblate"), ('\u{e8e3}', "dev-webpack"), ('\u{f070f}', "md-search-web"), ('\u{f1316}', "md-semantic-web")],
            DemonForm => &[('\u{ef76}', "fa-fire-flame-curved"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox"), ('\u{ef43}', "fa-dumpster-fire"), ('\u{f06d}', "fa-fire"), ('\u{f134}', "fa-fire-extinguisher"), ('\u{ed78}', "fa-fire-flame-simple"), ('\u{f269}', "fa-firefox"), ('\u{f2e2}', "fa-gripfire"), ('\u{f0edd}', "md-campfire")],
            Metallicize => &[('\u{ed25}', "fa-shield-halved"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline"), ('\u{f0498}', "md-shield"), ('\u{f088f}', "md-shield-account"), ('\u{f0a12}', "md-shield-account-outline"), ('\u{f15a7}', "md-shield-account-variant")],
            FeelNoPain => &[('\u{ed74}', "fa-bandage"), ('\u{f0daf}', "md-bandage"), ('\u{eb05}', "cod-heart"), ('\u{ec04}', "cod-heart-filled"), ('\u{ee4d}', "fa-face-grin-hearts"), ('\u{ee61}', "fa-face-kiss-wink-heart"), ('\u{ed9b}', "fa-hand-holding-heart"), ('\u{f004}', "fa-heart"), ('\u{ef4f}', "fa-heart-crack"), ('\u{f08a}', "fa-heart-o")],
            DarkEmbrace => &[('\u{f186}', "fa-moon-o"), ('\u{f1829}', "md-shield-moon-outline"), ('\u{eeef}', "fa-cloud-moon"), ('\u{ef1b}', "fa-cloud-moon-rain"), ('\u{e226}', "fae-moon-cloud"), ('\u{f0f61}', "md-moon-first-quarter"), ('\u{f0f62}', "md-moon-full"), ('\u{f0f63}', "md-moon-last-quarter"), ('\u{f0f64}', "md-moon-new"), ('\u{f0f65}', "md-moon-waning-crescent")],
            Evolve => &[('\u{f0638}', "md-cards"), ('\u{f08ce}', "md-cards-club"), ('\u{f189f}', "md-cards-club-outline"), ('\u{f08cf}', "md-cards-diamond"), ('\u{f101d}', "md-cards-diamond-outline"), ('\u{f0639}', "md-cards-outline"), ('\u{f18a1}', "md-cards-playing"), ('\u{f18a2}', "md-cards-playing-club"), ('\u{f18a3}', "md-cards-playing-club-multiple"), ('\u{f18a4}', "md-cards-playing-club-multiple-outline")],
            FireBreathing => &[('\u{f06d}', "fa-fire"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox"), ('\u{ef43}', "fa-dumpster-fire"), ('\u{f134}', "fa-fire-extinguisher"), ('\u{ef76}', "fa-fire-flame-curved"), ('\u{ed78}', "fa-fire-flame-simple"), ('\u{f269}', "fa-firefox"), ('\u{f2e2}', "fa-gripfire"), ('\u{f0edd}', "md-campfire")],
            Barricade => &[('\u{f07fe}', "md-wall"), ('\u{f1ee}', "fa-google-wallet"), ('\u{ee1e}', "fa-wallet"), ('\u{e25e}', "fae-wallet"), ('\u{f1a11}', "md-wall-fire"), ('\u{f091c}', "md-wall-sconce"), ('\u{f091d}', "md-wall-sconce-flat"), ('\u{f17c9}', "md-wall-sconce-flat-outline"), ('\u{f041c}', "md-wall-sconce-flat-variant"), ('\u{f17ca}', "md-wall-sconce-flat-variant-outline")],
            Brutality => &[('\u{f0cec}', "md-blood-bag"), ('\u{ee8e}', "fa-droplet-slash"), ('\u{ed9d}', "fa-hand-holding-droplet"), ('\u{eb05}', "cod-heart"), ('\u{ec04}', "cod-heart-filled"), ('\u{ee4d}', "fa-face-grin-hearts"), ('\u{ee61}', "fa-face-kiss-wink-heart"), ('\u{ed9b}', "fa-hand-holding-heart"), ('\u{f004}', "fa-heart"), ('\u{ef4f}', "fa-heart-crack")],
            Rupture => &[('\u{ee8e}', "fa-droplet-slash"), ('\u{ed9d}', "fa-hand-holding-droplet"), ('\u{f0cec}', "md-blood-bag"), ('\u{ed8d}', "fa-syringe"), ('\u{e7dc}', "dev-flask"), ('\u{f0c3}', "fa-flask"), ('\u{f0093}', "md-flask"), ('\u{f0094}', "md-flask-empty"), ('\u{f123a}', "md-flask-empty-minus"), ('\u{f123b}', "md-flask-empty-minus-outline")],
            Regenerate => &[('\u{f05f6}', "md-heart-pulse"), ('\u{f21e}', "fa-heartbeat"), ('\u{eb05}', "cod-heart"), ('\u{ec04}', "cod-heart-filled"), ('\u{ee4d}', "fa-face-grin-hearts"), ('\u{ee61}', "fa-face-kiss-wink-heart"), ('\u{ed9b}', "fa-hand-holding-heart"), ('\u{f004}', "fa-heart"), ('\u{ef4f}', "fa-heart-crack"), ('\u{f08a}', "fa-heart-o")],
            Thorns => &[('\u{f0db5}', "md-cactus"), ('\u{e22f}', "fae-plant"), ('\u{ead2}', "cod-debug-restart"), ('\u{eb90}', "cod-debug-restart-frame"), ('\u{ead3}', "cod-debug-start"), ('\u{ea6a}', "cod-star-empty"), ('\u{eb59}', "cod-star-full"), ('\u{eb5a}', "cod-star-half"), ('\u{ee50}', "fa-face-grin-stars"), ('\u{f251}', "fa-hourglass-start")],
            Berserk => &[('\u{ee50}', "fa-face-grin-stars"), ('\u{eaf2}', "cod-flame"), ('\u{ef76}', "fa-fire-flame-curved"), ('\u{ed78}', "fa-fire-flame-simple"), ('\u{f490}', "oct-flame"), ('\u{e0c0}', "ple-flame-thick"), ('\u{e0c2}', "ple-flame-thick-mirrored"), ('\u{e0c1}', "ple-flame-thin"), ('\u{e0c3}', "ple-flame-thin-mirrored"), ('\u{e787}', "dev-firebase")],
            Combust => &[('\u{ed78}', "fa-fire-flame-simple"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox"), ('\u{ef43}', "fa-dumpster-fire"), ('\u{f06d}', "fa-fire"), ('\u{f134}', "fa-fire-extinguisher"), ('\u{ef76}', "fa-fire-flame-curved"), ('\u{f269}', "fa-firefox"), ('\u{f2e2}', "fa-gripfire"), ('\u{f0edd}', "md-campfire")],
            Corruption => &[('\u{ee15}', "fa-skull"), ('\u{eeeb}', "fa-book-skull"), ('\u{ef0e}', "fa-skull-crossbones"), ('\u{f1132}', "md-bottle-tonic-skull"), ('\u{f1133}', "md-bottle-tonic-skull-outline"), ('\u{f068c}', "md-skull"), ('\u{f0bc6}', "md-skull-crossbones"), ('\u{f0bc7}', "md-skull-crossbones-outline"), ('\u{f0bc8}', "md-skull-outline"), ('\u{f14c7}', "md-skull-scan")],
            DoubleTap => &[('\u{f0787}', "md-sword-cross"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword"), ('\u{f18bf}', "md-shield-sword-outline"), ('\u{f04e5}', "md-sword"), ('\u{f01e}', "fa-repeat"), ('\u{f0b6}', "fa-repeat-alt"), ('\u{f0456}', "md-repeat"), ('\u{f0457}', "md-repeat-off")],
            Juggernaut => &[('\u{ef84}', "fa-helmet-safety"), ('\u{f0dc0}', "md-diving-helmet"), ('\u{f025f}', "md-football-helmet"), ('\u{f0d93}', "md-racing-helmet"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield"), ('\u{ed25}', "fa-shield-halved"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline")],
            Rage => &[('\u{ee1f}', "fa-face-angry"), ('\u{f0bc}', "fa-angrycreative"), ('\u{f0c69}', "md-emoticon-angry"), ('\u{f0c6a}', "md-emoticon-angry-outline"), ('\u{f169d}', "md-robot-angry"), ('\u{f169e}', "md-robot-angry-outline"), ('\u{ee48}', "fa-face-grimace"), ('\u{ee50}', "fa-face-grin-stars")],
            FlameBarrier => &[('\u{e0c0}', "ple-flame-thick"), ('\u{e0c2}', "ple-flame-thick-mirrored"), ('\u{eaf2}', "cod-flame"), ('\u{ef76}', "fa-fire-flame-curved"), ('\u{ed78}', "fa-fire-flame-simple"), ('\u{f490}', "oct-flame"), ('\u{e0c1}', "ple-flame-thin"), ('\u{e0c3}', "ple-flame-thin-mirrored"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox")],
            Artifact => &[('\u{f219}', "fa-gem"), ('\u{f0a81}', "md-zodiac-gemini"), ('\u{f29f}', "fa-diamond"), ('\u{eea2}', "fa-diamond-turn-right"), ('\u{f15ec}', "md-baseball-diamond"), ('\u{f15ed}', "md-baseball-diamond-outline"), ('\u{f08cf}', "md-cards-diamond"), ('\u{f101d}', "md-cards-diamond-outline"), ('\u{f18a6}', "md-cards-playing-diamond"), ('\u{f18a7}', "md-cards-playing-diamond-multiple")],
            NoBlock => &[('\u{f05e}', "fa-ban"), ('\u{e818}', "dev-kibana"), ('\u{ee16}', "fa-ban-smoking"), ('\u{ed74}', "fa-bandage"), ('\u{f2d5}', "fa-bandcamp"), ('\u{eda3}', "fa-piggy-bank"), ('\u{e281}', "fae-banana"), ('\u{f0daf}', "md-bandage"), ('\u{f0070}', "md-bank"), ('\u{f1655}', "md-bank-check")],
            NoDraw => &[('\u{eabd}', "cod-circle-slash"), ('\u{f468}', "oct-circle-slash"), ('\u{e818}', "dev-kibana"), ('\u{f05e}', "fa-ban"), ('\u{ee16}', "fa-ban-smoking"), ('\u{ed74}', "fa-bandage"), ('\u{f2d5}', "fa-bandcamp"), ('\u{eda3}', "fa-piggy-bank"), ('\u{e281}', "fae-banana"), ('\u{f0daf}', "md-bandage")],
            Mayhem => &[('\u{f074}', "fa-shuffle"), ('\u{f049d}', "md-shuffle"), ('\u{f049e}', "md-shuffle-disabled"), ('\u{f049f}', "md-shuffle-variant"), ('\u{edec}', "fa-dice"), ('\u{eef5}', "fa-dice-d20"), ('\u{eef6}', "fa-dice-d6"), ('\u{eded}', "fa-dice-five"), ('\u{edee}', "fa-dice-four"), ('\u{edef}', "fa-dice-one")],
            Magnetism => &[('\u{f076}', "fa-magnet"), ('\u{ebae}', "cod-magnet"), ('\u{f0347}', "md-magnet"), ('\u{f0348}', "md-magnet-on")],
            Panache => &[('\u{edf7}', "fa-feather"), ('\u{e7d8}', "dev-feathersjs"), ('\u{ee34}', "fa-feather-pointed"), ('\u{f06d3}', "md-feather"), ('\u{e7f8}', "dev-hardhat"), ('\u{e7bb}', "dev-redhat"), ('\u{efb7}', "fa-hat-cowboy"), ('\u{efb8}', "fa-hat-cowboy-side"), ('\u{ef01}', "fa-hat-wizard"), ('\u{edb0}', "fa-pied-piper-hat")],
            SadisticNature => &[('\u{ee53}', "fa-face-grin-tongue-squint"), ('\u{ee49}', "fa-face-grin"), ('\u{ee4b}', "fa-face-grin-beam"), ('\u{ee4c}', "fa-face-grin-beam-sweat"), ('\u{ee4d}', "fa-face-grin-hearts"), ('\u{ee4e}', "fa-face-grin-squint"), ('\u{ee4f}', "fa-face-grin-squint-tears"), ('\u{ee50}', "fa-face-grin-stars"), ('\u{ee51}', "fa-face-grin-tears"), ('\u{ee52}', "fa-face-grin-tongue")],
            PlatedArmor => &[('\u{f0ada}', "md-shield-plus"), ('\u{f0adb}', "md-shield-plus-outline"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield"), ('\u{ed25}', "fa-shield-halved"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline"), ('\u{f0498}', "md-shield"), ('\u{f088f}', "md-shield-account")],
            Constricted => &[('\u{f150e}', "md-snake"), ('\u{ec16}', "cod-snake"), ('\u{f1ca}', "fa-vine"), ('\u{eb8d}', "cod-symbol-string"), ('\u{e8ac}', "dev-spring"), ('\u{f2a4}', "fa-hard-of-hearing"), ('\u{ef0b}', "fa-ring"), ('\u{ed8d}', "fa-syringe"), ('\u{e23d}', "fae-ring"), ('\u{f003d}', "md-arrange-bring-forward")],
            Confused => &[('\u{f02d7}', "md-help-circle"), ('\u{f0625}', "md-help-circle-outline"), ('\u{eb32}', "cod-question"), ('\u{f128}', "fa-question"), ('\u{f059}', "fa-question-circle"), ('\u{f29c}', "fa-question-circle-o"), ('\u{f0b59}', "md-account-question"), ('\u{f0b5a}', "md-account-question-outline"), ('\u{f1231}', "md-beaker-question"), ('\u{f1232}', "md-beaker-question-outline")],
            Hex => &[('\u{ef21}', "fa-hurricane"), ('\u{f0898}', "md-weather-hurricane"), ('\u{e36c}', "weather-hurricane"), ('\u{e3c7}', "weather-hurricane-warning"), ('\u{e24f}', "fae-hexagon"), ('\u{f0049}', "md-arrow-down-bold-hexagon-outline"), ('\u{f0051}', "md-arrow-left-bold-hexagon-outline"), ('\u{f0058}', "md-arrow-right-bold-hexagon-outline"), ('\u{f0061}', "md-arrow-up-bold-hexagon-outline"), ('\u{f15ff}', "md-dots-hexagon")],
            DrawReduction => &[('\u{f0639}', "md-cards-outline"), ('\u{f0638}', "md-cards"), ('\u{f08ce}', "md-cards-club"), ('\u{f189f}', "md-cards-club-outline"), ('\u{f08cf}', "md-cards-diamond"), ('\u{f101d}', "md-cards-diamond-outline"), ('\u{f18a1}', "md-cards-playing"), ('\u{f18a2}', "md-cards-playing-club"), ('\u{f18a3}', "md-cards-playing-club-multiple"), ('\u{f18a4}', "md-cards-playing-club-multiple-outline")],
            Surrounded => &[('\u{f0849}', "md-account-group"), ('\u{f0b58}', "md-account-group-outline"), ('\u{f0c0}', "fa-users"), ('\u{edd3}', "fa-users-gear"), ('\u{eda2}', "fa-people-carry-box"), ('\u{f038f}', "md-nature-people"), ('\u{f4fd}', "oct-people"), ('\u{f07e}', "fa-arrows-left-right"), ('\u{f047}', "fa-arrows-up-down-left-right"), ('\u{f07d}', "fa-arrows-v")],
            Intangible => &[('\u{f02a0}', "md-ghost"), ('\u{e71f}', "dev-ghost"), ('\u{e714}', "dev-ghost-small"), ('\u{eefe}', "fa-ghost"), ('\u{f2ac}', "fa-snapchat-ghost"), ('\u{f09f5}', "md-ghost-off"), ('\u{f165c}', "md-ghost-off-outline"), ('\u{f165d}', "md-ghost-outline"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield")],
            Ritual => &[('\u{eedb}', "fa-hands-praying"), ('\u{eeda}', "fa-person-praying"), ('\u{ee84}', "fa-spray-can"), ('\u{ee97}', "fa-spray-can-sparkles"), ('\u{f0579}', "md-hands-pray"), ('\u{f0665}', "md-spray"), ('\u{f0ae0}', "md-spray-bottle"), ('\u{f1fd}', "fa-cake-candles"), ('\u{f05e2}', "md-candle"), ('\u{ede7}', "fa-church")],
            Enrage => &[('\u{f0a1}', "fa-bullhorn"), ('\u{f00e6}', "md-bullhorn"), ('\u{f0b23}', "md-bullhorn-outline"), ('\u{f196e}', "md-bullhorn-variant"), ('\u{f196f}', "md-bullhorn-variant-outline"), ('\u{eb1e}', "cod-megaphone"), ('\u{f45f}', "oct-megaphone"), ('\u{f0bc}', "fa-angrycreative"), ('\u{ee1f}', "fa-face-angry"), ('\u{f0c69}', "md-emoticon-angry")],
            Anger => &[('\u{ee48}', "fa-face-grimace"), ('\u{ee1f}', "fa-face-angry"), ('\u{f0bc}', "fa-angrycreative"), ('\u{f0c69}', "md-emoticon-angry"), ('\u{f0c6a}', "md-emoticon-angry-outline"), ('\u{f169d}', "md-robot-angry"), ('\u{f169e}', "md-robot-angry-outline"), ('\u{ee43}', "fa-face-frown-open"), ('\u{ef2c}', "fa-cloud-bolt"), ('\u{f0db3}', "md-bolt")],
            SharpHide => &[('\u{eefd}', "fa-hand-fist"), ('\u{ed25}', "fa-shield-halved"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword"), ('\u{f18bf}', "md-shield-sword-outline"), ('\u{f04e5}', "md-sword"), ('\u{f0787}', "md-sword-cross"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield")],
            BeatOfDeath => &[('\u{f21e}', "fa-heartbeat"), ('\u{f05f6}', "md-heart-pulse"), ('\u{eb05}', "cod-heart"), ('\u{ec04}', "cod-heart-filled"), ('\u{ee4d}', "fa-face-grin-hearts"), ('\u{ee61}', "fa-face-kiss-wink-heart"), ('\u{ed9b}', "fa-hand-holding-heart"), ('\u{f004}', "fa-heart"), ('\u{ef4f}', "fa-heart-crack"), ('\u{f08a}', "fa-heart-o")],
            Curiosity => &[('\u{f0349}', "md-magnify"), ('\u{eedd}', "fa-magnifying-glass-dollar"), ('\u{eede}', "fa-magnifying-glass-location"), ('\u{f0980}', "md-magnify-close"), ('\u{f1874}', "md-magnify-expand"), ('\u{f034a}', "md-magnify-minus"), ('\u{f0a62}', "md-magnify-minus-cursor"), ('\u{f06ec}', "md-magnify-minus-outline"), ('\u{f034b}', "md-magnify-plus"), ('\u{f0a63}', "md-magnify-plus-cursor")],
            CurlUp => &[('\u{f0adb}', "md-shield-plus-outline"), ('\u{eb53}', "cod-shield"), ('\u{f132}', "fa-shield"), ('\u{ed25}', "fa-shield-halved"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline"), ('\u{f0498}', "md-shield"), ('\u{f088f}', "md-shield-account"), ('\u{f0a12}', "md-shield-account-outline")],
            Explosive => &[('\u{f0691}', "md-bomb"), ('\u{f1e2}', "fa-bomb"), ('\u{f06c5}', "md-bomb-off"), ('\u{ee9f}', "fa-car-burst"), ('\u{f0693}', "md-camera-burst"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox"), ('\u{ef43}', "fa-dumpster-fire"), ('\u{f06d}', "fa-fire"), ('\u{f134}', "fa-fire-extinguisher")],
            Fading => &[('\u{f253}', "fa-hourglass-end"), ('\u{f254}', "fa-hourglass"), ('\u{f252}', "fa-hourglass-half"), ('\u{f250}', "fa-hourglass-o"), ('\u{f251}', "fa-hourglass-start"), ('\u{f4e3}', "oct-hourglass"), ('\u{e71f}', "dev-ghost"), ('\u{e714}', "dev-ghost-small"), ('\u{eefe}', "fa-ghost"), ('\u{f2ac}', "fa-snapchat-ghost")],
            Flight => &[('\u{ed99}', "fa-dove"), ('\u{e7d8}', "dev-feathersjs"), ('\u{edf7}', "fa-feather"), ('\u{ee34}', "fa-feather-pointed"), ('\u{f06d3}', "md-feather"), ('\u{e201}', "fae-snowing"), ('\u{f01de}', "md-drawing"), ('\u{f01df}', "md-drawing-box"), ('\u{f0608}', "md-rowing"), ('\u{edff}', "fa-kiwi-bird")],
            Invincible => &[('\u{f132}', "fa-shield"), ('\u{eb53}', "cod-shield"), ('\u{ed25}', "fa-shield-halved"), ('\u{edcf}', "fa-user-shield"), ('\u{f101b}', "md-car-windshield"), ('\u{f101c}', "md-car-windshield-outline"), ('\u{f0498}', "md-shield"), ('\u{f088f}', "md-shield-account"), ('\u{f0a12}', "md-shield-account-outline"), ('\u{f15a7}', "md-shield-account-variant")],
            Malleable => &[('\u{eeb0}', "fa-shapes"), ('\u{edec}', "fa-dice"), ('\u{eef5}', "fa-dice-d20"), ('\u{eef6}', "fa-dice-d6"), ('\u{eded}', "fa-dice-five"), ('\u{edee}', "fa-dice-four"), ('\u{edef}', "fa-dice-one"), ('\u{edf0}', "fa-dice-six"), ('\u{edf1}', "fa-dice-three"), ('\u{edf2}', "fa-dice-two")],
            Minion => &[('\u{f0c0}', "fa-users"), ('\u{edd3}', "fa-users-gear"), ('\u{ede6}', "fa-chalkboard-user"), ('\u{ef7c}', "fa-clipboard-user"), ('\u{ef86}', "fa-hospital-user"), ('\u{f007}', "fa-user"), ('\u{edc5}', "fa-user-astronaut"), ('\u{edc6}', "fa-user-check"), ('\u{f2bd}', "fa-user-circle"), ('\u{f2be}', "fa-user-circle-o")],
            MinionLeader => &[('\u{edeb}', "fa-crown"), ('\u{e26e}', "fae-crown"), ('\u{f01a5}', "md-crown"), ('\u{f17dc}', "md-crown-circle"), ('\u{f17dd}', "md-crown-circle-outline"), ('\u{f11d0}', "md-crown-outline"), ('\u{f18bc}', "md-shield-crown"), ('\u{f18bd}', "md-shield-crown-outline"), ('\u{ed62}', "fa-chess-king"), ('\u{e260}', "fae-chess-king")],
            ModeShift => &[('\u{f2f1}', "fa-rotate"), ('\u{f2ea}', "fa-rotate-left"), ('\u{f2f9}', "fa-rotate-right"), ('\u{f0d4e}', "md-axis-x-rotate-clockwise"), ('\u{f0d4f}', "md-axis-x-rotate-counterclockwise"), ('\u{f0d53}', "md-axis-y-rotate-clockwise"), ('\u{f0d54}', "md-axis-y-rotate-counterclockwise"), ('\u{f0d57}', "md-axis-z-rotate-clockwise"), ('\u{f0d58}', "md-axis-z-rotate-counterclockwise"), ('\u{f0696}', "md-crop-rotate")],
            PainfulStabs => &[('\u{ed8d}', "fa-syringe"), ('\u{e2a1}', "fae-injection"), ('\u{f0391}', "md-needle"), ('\u{f19d2}', "md-needle-off"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword"), ('\u{f18bf}', "md-shield-sword-outline"), ('\u{f04e5}', "md-sword"), ('\u{f0787}', "md-sword-cross")],
            Reactive => &[('\u{f0db3}', "md-bolt"), ('\u{ef2c}', "fa-cloud-bolt"), ('\u{f1903}', "md-home-lightning-bolt"), ('\u{f1904}', "md-home-lightning-bolt-outline"), ('\u{f140b}', "md-lightning-bolt"), ('\u{f0820}', "md-lightning-bolt-circle"), ('\u{f140c}', "md-lightning-bolt-outline"), ('\u{f0593}', "md-weather-lightning"), ('\u{f067e}', "md-weather-lightning-rainy"), ('\u{f0f32}', "md-weather-partly-lightning")],
            Regrow => &[('\u{eda7}', "fa-seedling"), ('\u{e22f}', "fae-plant"), ('\u{ef39}', "fa-canadian-maple-leaf"), ('\u{f06c}', "fa-leaf"), ('\u{f032a}', "md-leaf"), ('\u{f1905}', "md-leaf-circle"), ('\u{f1906}', "md-leaf-circle-outline"), ('\u{f0c93}', "md-leaf-maple"), ('\u{f12da}', "md-leaf-maple-off"), ('\u{f12d9}', "md-leaf-off")],
            Shifting => &[('\u{f07d}', "fa-arrows-v"), ('\u{f047}', "fa-arrows-up-down-left-right"), ('\u{f07e}', "fa-arrows-left-right"), ('\u{f0ec}', "fa-exchange"), ('\u{f18d}', "fa-stack-exchange"), ('\u{f060b}', "md-stack-exchange"), ('\u{eb37}', "cod-refresh"), ('\u{f021}', "fa-refresh"), ('\u{f1765}', "md-archive-refresh"), ('\u{f1766}', "md-archive-refresh-outline")],
            Slow => &[('\u{f017}', "fa-clock-o"), ('\u{f0b57}', "md-account-clock-outline"), ('\u{f1750}', "md-archive-clock-outline"), ('\u{f19e6}', "md-battery-clock-outline"), ('\u{f1689}', "md-book-clock-outline"), ('\u{f10d1}', "md-briefcase-clock-outline"), ('\u{f16e1}', "md-calendar-clock-outline"), ('\u{f16e3}', "md-clipboard-clock-outline"), ('\u{f18fa}', "md-clipboard-text-clock-outline"), ('\u{f0154}', "md-clock-out")],
            SporeCloud => &[('\u{f0c2}', "fa-cloud"), ('\u{ebaa}', "cod-cloud"), ('\u{eac2}', "cod-cloud-download"), ('\u{eac3}', "cod-cloud-upload"), ('\u{e792}', "dev-cloudflare"), ('\u{e793}', "dev-cloudflareworkers"), ('\u{e7f1}', "dev-googlecloud"), ('\u{ef2c}', "fa-cloud-bolt"), ('\u{f0ed}', "fa-cloud-download"), ('\u{ef1a}', "fa-cloud-meatball")],
            Split => &[('\u{f062c}', "md-source-branch"), ('\u{f14cf}', "md-source-branch-check"), ('\u{f14cb}', "md-source-branch-minus"), ('\u{f14ca}', "md-source-branch-plus"), ('\u{f14cd}', "md-source-branch-refresh"), ('\u{f14cc}', "md-source-branch-remove"), ('\u{f14ce}', "md-source-branch-sync"), ('\u{eb3e}', "cod-repo-clone"), ('\u{f24d}', "fa-clone"), ('\u{f43f}', "oct-repo-clone")],
            Stasis => &[('\u{f187}', "fa-box-archive"), ('\u{f0765}', "checkbox-blank-circle"), ('\u{f043d}', "checkbox-blank-circle-outline"), ('\u{f0766}', "checkbox-blank-circle-outline.1"), ('\u{eb09}', "cod-inbox"), ('\u{e707}', "dev-dropbox"), ('\u{ed75}', "fa-box"), ('\u{ed95}', "fa-box-open"), ('\u{ed76}', "fa-boxes-stacked"), ('\u{f16b}', "fa-dropbox")],
            StrengthUp => &[('\u{f102}', "fa-angles-up"), ('\u{eaa1}', "cod-arrow-up"), ('\u{f062}', "fa-arrow-up"), ('\u{efb2}', "fa-arrow-up-9-1"), ('\u{efb0}', "fa-arrow-up-short-wide"), ('\u{efae}', "fa-arrow-up-z-a"), ('\u{f0aa}', "fa-circle-arrow-up"), ('\u{ee3d}', "fa-file-arrow-up"), ('\u{f176}', "fa-long-arrow-up"), ('\u{f14c}', "fa-square-arrow-up-right")],
            TimeWarp => &[('\u{f254}', "fa-hourglass"), ('\u{f253}', "fa-hourglass-end"), ('\u{f252}', "fa-hourglass-half"), ('\u{f250}', "fa-hourglass-o"), ('\u{f251}', "fa-hourglass-start"), ('\u{f4e3}', "oct-hourglass"), ('\u{f017}', "fa-clock-o"), ('\u{edc7}', "fa-user-clock"), ('\u{f0b56}', "md-account-clock"), ('\u{f0b57}', "md-account-clock-outline")],
            Asleep => &[('\u{f04b2}', "md-sleep"), ('\u{23fe}', "iec-sleep-mode"), ('\u{f00a0}', "md-bell-sleep"), ('\u{f0a93}', "md-bell-sleep-outline"), ('\u{f12d1}', "md-chat-sleep"), ('\u{f12d2}', "md-chat-sleep-outline"), ('\u{f0904}', "md-power-sleep"), ('\u{f04b3}', "md-sleep-off"), ('\u{eeef}', "fa-cloud-moon"), ('\u{ef1b}', "fa-cloud-moon-rain")],
            Duplication => &[('\u{f24d}', "fa-clone"), ('\u{eb3e}', "cod-repo-clone"), ('\u{f43f}', "oct-repo-clone"), ('\u{ebcc}', "cod-copy"), ('\u{f1f9}', "fa-copyright"), ('\u{f018f}', "md-content-copy"), ('\u{f1939}', "md-copyleft"), ('\u{f05e6}', "md-copyright"), ('\u{f4bb}', "oct-copy"), ('\u{f01e}', "fa-repeat")],
            LoseStrength => &[('\u{f063}', "fa-arrow-down"), ('\u{ea9a}', "cod-arrow-down"), ('\u{efb1}', "fa-arrow-down-9-1"), ('\u{efaf}', "fa-arrow-down-short-wide"), ('\u{efad}', "fa-arrow-down-z-a"), ('\u{f218}', "fa-cart-arrow-down"), ('\u{f0ab}', "fa-circle-arrow-down"), ('\u{ee36}', "fa-file-arrow-down"), ('\u{f175}', "fa-long-arrow-down"), ('\u{f1868}', "md-account-arrow-down")],
            LoseDexterity => &[('\u{ea9a}', "cod-arrow-down"), ('\u{f063}', "fa-arrow-down"), ('\u{efb1}', "fa-arrow-down-9-1"), ('\u{efaf}', "fa-arrow-down-short-wide"), ('\u{efad}', "fa-arrow-down-z-a"), ('\u{f218}', "fa-cart-arrow-down"), ('\u{f0ab}', "fa-circle-arrow-down"), ('\u{ee36}', "fa-file-arrow-down"), ('\u{f175}', "fa-long-arrow-down"), ('\u{f1868}', "md-account-arrow-down")],
            Focus => &[('\u{f140}', "fa-bullseye"), ('\u{f05dd}', "md-bullseye"), ('\u{f08c9}', "md-bullseye-arrow"), ('\u{f05b}', "fa-crosshairs"), ('\u{eea9}', "fa-location-crosshairs"), ('\u{f01a3}', "md-crosshairs"), ('\u{f01a4}', "md-crosshairs-gps"), ('\u{f0f45}', "md-crosshairs-off"), ('\u{f1136}', "md-crosshairs-question"), ('\u{ebf8}', "cod-target")],
            Poison => &[('\u{ef0e}', "fa-skull-crossbones"), ('\u{f0bc6}', "md-skull-crossbones"), ('\u{f0bc7}', "md-skull-crossbones-outline"), ('\u{e7dc}', "dev-flask"), ('\u{f0c3}', "fa-flask"), ('\u{f0093}', "md-flask"), ('\u{f0094}', "md-flask-empty"), ('\u{f123a}', "md-flask-empty-minus"), ('\u{f123b}', "md-flask-empty-minus-outline"), ('\u{f13f4}', "md-flask-empty-off")],
            Accuracy => &[('\u{f05b}', "fa-crosshairs"), ('\u{eea9}', "fa-location-crosshairs"), ('\u{f01a3}', "md-crosshairs"), ('\u{f01a4}', "md-crosshairs-gps"), ('\u{f0f45}', "md-crosshairs-off"), ('\u{f1136}', "md-crosshairs-question"), ('\u{f140}', "fa-bullseye"), ('\u{f05dd}', "md-bullseye"), ('\u{f08c9}', "md-bullseye-arrow"), ('\u{ebf8}', "cod-target")],
            AfterImage => &[('\u{f00f}', "fa-images"), ('\u{ed19}', "fa-image-portrait"), ('\u{f0509}', "image-filter-hdr"), ('\u{f08cc}', "md-camera-image"), ('\u{f021f}', "md-file-image"), ('\u{f1772}', "md-file-image-marker"), ('\u{f1773}', "md-file-image-marker-outline"), ('\u{f193b}', "md-file-image-minus"), ('\u{f193c}', "md-file-image-minus-outline"), ('\u{f0eb0}', "md-file-image-outline")],
            ThousandCuts => &[('\u{f09fb}', "md-knife"), ('\u{f09fc}', "md-knife-military"), ('\u{f0a70}', "md-silverware-fork-knife"), ('\u{f0c86}', "md-fuse-blade"), ('\u{f0d2c}', "md-rollerblade"), ('\u{f002e}', "md-rollerblade-off"), ('\u{f0e61}', "md-saw-blade"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword")],
            Envenom => &[('\u{f0c3}', "fa-flask"), ('\u{e7dc}', "dev-flask"), ('\u{f0093}', "md-flask"), ('\u{f0094}', "md-flask-empty"), ('\u{f123a}', "md-flask-empty-minus"), ('\u{f123b}', "md-flask-empty-minus-outline"), ('\u{f13f4}', "md-flask-empty-off"), ('\u{f13f5}', "md-flask-empty-off-outline"), ('\u{f0095}', "md-flask-empty-outline"), ('\u{f123c}', "md-flask-empty-plus")],
            InfiniteBlades => &[('\u{eecf}', "fa-khanda"), ('\u{f10fd}', "md-khanda"), ('\u{f07f5}', "md-form-textbox-password"), ('\u{f0881}', "md-onepassword"), ('\u{f18be}', "md-shield-sword"), ('\u{f18bf}', "md-shield-sword-outline"), ('\u{f04e5}', "md-sword"), ('\u{f0787}', "md-sword-cross"), ('\u{f09fb}', "md-knife"), ('\u{f09fc}', "md-knife-military")],
            NoxiousFumes => &[('\u{ef29}', "fa-smog"), ('\u{f0a71}', "md-smog"), ('\u{e36d}', "weather-smog"), ('\u{ebaa}', "cod-cloud"), ('\u{eac2}', "cod-cloud-download"), ('\u{eac3}', "cod-cloud-upload"), ('\u{e792}', "dev-cloudflare"), ('\u{e793}', "dev-cloudflareworkers"), ('\u{e7f1}', "dev-googlecloud"), ('\u{f0c2}', "fa-cloud")],
            ToolsOfTheTrade => &[('\u{ef70}', "fa-screwdriver-wrench"), ('\u{ee1b}', "fa-toolbox"), ('\u{f09ac}', "md-toolbox"), ('\u{f09ad}', "md-toolbox-outline"), ('\u{f0ad}', "fa-wrench"), ('\u{f189a}', "md-account-wrench"), ('\u{f189b}', "md-account-wrench-outline"), ('\u{f1814}', "md-car-wrench"), ('\u{f19fc}', "md-folder-wrench"), ('\u{f19fd}', "md-folder-wrench-outline")],
            WraithForm => &[('\u{eefe}', "fa-ghost"), ('\u{e71f}', "dev-ghost"), ('\u{e714}', "dev-ghost-small"), ('\u{f2ac}', "fa-snapchat-ghost"), ('\u{f02a0}', "md-ghost"), ('\u{f09f5}', "md-ghost-off"), ('\u{f165c}', "md-ghost-off-outline"), ('\u{f165d}', "md-ghost-outline"), ('\u{f14f1}', "md-spirit-level"), ('\u{eb53}', "cod-shield")],
            Blur => &[('\u{f070}', "fa-eye-slash"), ('\u{f2a8}', "fa-low-vision"), ('\u{ea70}', "cod-eye"), ('\u{eae7}', "cod-eye-closed"), ('\u{f140}', "fa-bullseye"), ('\u{f06e}', "fa-eye"), ('\u{f1fb}', "fa-eyedropper"), ('\u{ee6e}', "fa-face-rolling-eyes"), ('\u{f0420}', "md-account-eye"), ('\u{f127b}', "md-account-eye-outline")],
            PhantasmalKiller => &[('\u{f21b}', "fa-user-secret"), ('\u{ef07}', "fa-mask"), ('\u{eeb6}', "fa-masks-theater"), ('\u{f1023}', "md-domino-mask"), ('\u{f0d02}', "md-drama-masks"), ('\u{f1586}', "md-face-mask"), ('\u{f1587}', "md-face-mask-outline"), ('\u{f0825}', "md-guy-fawkes-mask"), ('\u{f0916}', "md-transition-masked"), ('\u{e8ae}', "dev-spyder")],
            Burst => &[('\u{f04e}', "fa-forward"), ('\u{f050}', "fa-forward-fast"), ('\u{f051}', "fa-step-forward"), ('\u{f003d}', "md-arrange-bring-forward"), ('\u{f0211}', "md-fast-forward"), ('\u{f0d71}', "md-fast-forward-10"), ('\u{f193a}', "md-fast-forward-15"), ('\u{f0d06}', "md-fast-forward-30"), ('\u{f11f8}', "md-fast-forward-5"), ('\u{f160b}', "md-fast-forward-60")],
            WellLaidPlans => &[('\u{eab3}', "cod-checklist"), ('\u{e29a}', "fae-checklist-o"), ('\u{f45e}', "oct-checklist"), ('\u{f096a}', "md-format-list-checkbox"), ('\u{f0756}', "md-format-list-checks"), ('\u{f05c7}', "md-playlist-check"), ('\u{f07f}', "fa-clipboard-alt"), ('\u{ed7a}', "fa-clipboard-check"), ('\u{ed7b}', "fa-clipboard-list"), ('\u{ef7c}', "fa-clipboard-user")],
            Choke => &[('\u{f0a4f}', "md-hand-front-right"), ('\u{f182f}', "md-hand-front-right-outline"), ('\u{e7f7}', "dev-handlebars"), ('\u{ed73}', "fa-hand-dots"), ('\u{eefd}', "fa-hand-fist"), ('\u{ed9a}', "fa-hand-holding"), ('\u{ed9c}', "fa-hand-holding-dollar"), ('\u{ed9d}', "fa-hand-holding-droplet"), ('\u{ed9b}', "fa-hand-holding-heart"), ('\u{f258}', "fa-hand-lizard-o")],
            CorpseExplosion => &[('\u{f1e2}', "fa-bomb"), ('\u{f0691}', "md-bomb"), ('\u{f06c5}', "md-bomb-off"), ('\u{ef0e}', "fa-skull-crossbones"), ('\u{f0bc6}', "md-skull-crossbones"), ('\u{f0bc7}', "md-skull-crossbones-outline"), ('\u{e787}', "dev-firebase"), ('\u{e745}', "dev-firefox"), ('\u{ef43}', "fa-dumpster-fire"), ('\u{f06d}', "fa-fire")],
            DoubleDamage => &[('\u{ee29}', "fa-check-double"), ('\u{ef2c}', "fa-cloud-bolt"), ('\u{f0db3}', "md-bolt"), ('\u{f1903}', "md-home-lightning-bolt"), ('\u{f1904}', "md-home-lightning-bolt-outline"), ('\u{f140b}', "md-lightning-bolt"), ('\u{f0820}', "md-lightning-bolt-circle"), ('\u{f140c}', "md-lightning-bolt-outline"), ('\u{ed67}', "fa-dumbbell"), ('\u{f01e6}', "md-dumbbell")],
        }
    }

    /// 图鉴用的语料 id:语料里的能力名与我们多数能对上,对不上的别名写在这里
    pub fn power_id(self) -> &'static str {
        use Status::*;
        match self {
            SadisticNature => "sadistic",
            Anger => "angry",
            Choke => "choked",
            ThousandCuts => "thousand_cuts",
            PhantasmalKiller => "phantasmal",
            StrengthUp => "strength_up",
            Regrow => "regrow",
            WellLaidPlans => "well_laid_plans",
            other => other.name_key(),
        }
    }

    /// 语料里没有对应能力文案的少数几条,这里手写一句
    pub fn summary_fallback(self) -> &'static str {
        use Status::*;
        match self {
            Regrow => "半血复活一次的倒计时(暗灵)",
            StrengthUp => "每回合结束获得等量力量(圆球步行者)",
            WellLaidPlans => "回合结束时最多保留等量张手牌",
            _ => "",
        }
    }

    /// 纯标记类(引擎不读,只在界面上标一下):图鉴归到 markers 分组
    pub fn is_marker(self) -> bool {
        use Status::*;
        matches!(self, Reactive | Split | Stasis)
    }

    /// name() 转小写下划线(语料 id 的写法)
    fn name_key(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "strength",
            Dexterity => "dexterity",
            Vulnerable => "vulnerable",
            Weak => "weak",
            Frail => "frail",
            Entangled => "entangled",
            DemonForm => "demon_form",
            Metallicize => "metallicize",
            FeelNoPain => "feel_no_pain",
            DarkEmbrace => "dark_embrace",
            Evolve => "evolve",
            FireBreathing => "fire_breathing",
            Barricade => "barricade",
            Brutality => "brutality",
            Rupture => "rupture",
            Regenerate => "regenerate",
            Thorns => "thorns",
            Berserk => "berserk",
            Combust => "combust",
            Corruption => "corruption",
            DoubleTap => "double_tap",
            Juggernaut => "juggernaut",
            Rage => "rage",
            FlameBarrier => "flame_barrier",
            Artifact => "artifact",
            NoBlock => "no_block",
            NoDraw => "no_draw",
            Mayhem => "mayhem",
            Magnetism => "magnetism",
            Panache => "panache",
            SadisticNature => "sadistic_nature",
            PlatedArmor => "plated_armor",
            Constricted => "constricted",
            Confused => "confused",
            Hex => "hex",
            DrawReduction => "draw_reduction",
            Surrounded => "surrounded",
            Intangible => "intangible",
            Ritual => "ritual",
            Enrage => "enrage",
            Anger => "anger",
            SharpHide => "sharp_hide",
            BeatOfDeath => "beat_of_death",
            Curiosity => "curiosity",
            CurlUp => "curl_up",
            Explosive => "explosive",
            Fading => "fading",
            Flight => "flight",
            Invincible => "invincible",
            Malleable => "malleable",
            Minion => "minion",
            MinionLeader => "minion_leader",
            ModeShift => "mode_shift",
            PainfulStabs => "painful_stabs",
            Reactive => "reactive",
            Regrow => "regrow",
            Shifting => "shifting",
            Slow => "slow",
            SporeCloud => "spore_cloud",
            Split => "split",
            Stasis => "stasis",
            StrengthUp => "strength_up",
            TimeWarp => "time_warp",
            Asleep => "asleep",
            Duplication => "duplication",
            LoseStrength => "lose_strength",
            LoseDexterity => "lose_dexterity",
            Focus => "focus",
            Poison => "poison",
            Accuracy => "accuracy",
            AfterImage => "after_image",
            ThousandCuts => "a_thousand_cuts",
            Envenom => "envenom",
            InfiniteBlades => "infinite_blades",
            NoxiousFumes => "noxious_fumes",
            ToolsOfTheTrade => "tools_of_the_trade",
            WraithForm => "wraith_form",
            Blur => "blur",
            PhantasmalKiller => "phantasmal_killer",
            Burst => "burst",
            WellLaidPlans => "well_laid_plans",
            Choke => "choke",
            CorpseExplosion => "corpse_explosion",
            DoubleDamage => "double_damage",
        }
    }

    pub fn name(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "Strength",
            Dexterity => "Dexterity",
            Vulnerable => "Vulnerable",
            Weak => "Weak",
            Frail => "Frail",
            Entangled => "Entangled",
            DemonForm => "Demon Form",
            Metallicize => "Metallicize",
            FeelNoPain => "Feel No Pain",
            DarkEmbrace => "Dark Embrace",
            Evolve => "Evolve",
            FireBreathing => "Fire Breathing",
            Barricade => "Barricade",
            Brutality => "Brutality",
            Rupture => "Rupture",
            Regenerate => "Regenerate",
            Thorns => "Thorns",
            Berserk => "Berserk",
            Combust => "Combust",
            Corruption => "Corruption",
            DoubleTap => "Double Tap",
            Juggernaut => "Juggernaut",
            Rage => "Rage",
            FlameBarrier => "Flame Barrier",
            Artifact => "Artifact",
            NoBlock => "No Block",
            NoDraw => "No Draw",
            Mayhem => "Mayhem",
            Magnetism => "Magnetism",
            Panache => "Panache",
            SadisticNature => "Sadistic Nature",
            PlatedArmor => "Plated Armor",
            Constricted => "Constricted",
            Confused => "Confused",
            Hex => "Hex",
            DrawReduction => "Draw Reduction",
            Surrounded => "Surrounded",
            Intangible => "Intangible",
            Ritual => "Ritual",
            Enrage => "Enrage",
            Anger => "Anger",
            SharpHide => "Sharp Hide",
            BeatOfDeath => "Beat of Death",
            Curiosity => "Curiosity",
            CurlUp => "Curl Up",
            Explosive => "Explosive",
            Fading => "Fading",
            Flight => "Flight",
            Invincible => "Invincible",
            Malleable => "Malleable",
            Minion => "Minion",
            MinionLeader => "Minion Leader",
            ModeShift => "Mode Shift",
            PainfulStabs => "Painful Stabs",
            Reactive => "Reactive",
            Regrow => "Regrow",
            Shifting => "Shifting",
            Slow => "Slow",
            SporeCloud => "Spore Cloud",
            Split => "Split",
            Stasis => "Stasis",
            StrengthUp => "Strength Up",
            TimeWarp => "Time Warp",
            Asleep => "Asleep",
            Duplication => "Duplication",
            LoseStrength => "Lose Strength",
            LoseDexterity => "Lose Dexterity",
            Focus => "Focus",
            Poison => "Poison",
            Accuracy => "Accuracy",
            AfterImage => "After Image",
            ThousandCuts => "A Thousand Cuts",
            Envenom => "Envenom",
            InfiniteBlades => "Infinite Blades",
            NoxiousFumes => "Noxious Fumes",
            ToolsOfTheTrade => "Tools of the Trade",
            WraithForm => "Wraith Form",
            Blur => "Blur",
            PhantasmalKiller => "Phantasmal Killer",
            Burst => "Burst",
            WellLaidPlans => "Well-Laid Plans",
            Choke => "Choke",
            CorpseExplosion => "Corpse Explosion",
            DoubleDamage => "Double Damage",
        }
    }

    /// 状态行里用的短标签,控制在 4 个字符内
    pub fn short(self) -> &'static str {
        use Status::*;
        match self {
            Strength => "STR",
            Dexterity => "DEX",
            Vulnerable => "VULN",
            Weak => "WEAK",
            Frail => "FRAIL",
            Entangled => "ENTG",
            DemonForm => "DEMON",
            Metallicize => "METAL",
            FeelNoPain => "FNP",
            DarkEmbrace => "DARK",
            Evolve => "EVOLV",
            FireBreathing => "FIRE",
            Barricade => "BARR",
            Brutality => "BRUT",
            Rupture => "RUPT",
            Regenerate => "REGEN",
            Thorns => "THORN",
            Berserk => "BSRK",
            Combust => "CMBS",
            Corruption => "CORR",
            DoubleTap => "DBLT",
            Juggernaut => "JUGG",
            Rage => "RAGE",
            FlameBarrier => "FLMB",
            Artifact => "ARTF",
            NoBlock => "NBLK",
            NoDraw => "NDRW",
            Mayhem => "MAYH",
            Magnetism => "MAGN",
            Panache => "PAN",
            SadisticNature => "SAD",
            PlatedArmor => "PLATE",
            Constricted => "CSTR",
            Confused => "CNFS",
            Hex => "HEX",
            DrawReduction => "DRED",
            Surrounded => "SURR",
            Intangible => "INTG",
            Ritual => "RITUAL",
            Enrage => "ENRAGE",
            Anger => "ANGR",
            SharpHide => "HIDE",
            BeatOfDeath => "BEAT",
            Curiosity => "CURI",
            CurlUp => "CURL",
            Explosive => "XPLS",
            Fading => "FADE",
            Flight => "FLGT",
            Invincible => "INVC",
            Malleable => "MALL",
            Minion => "MINN",
            MinionLeader => "LEAD",
            ModeShift => "MODE",
            PainfulStabs => "PSTB",
            Reactive => "REAC",
            Regrow => "RGRW",
            Shifting => "SHFT",
            Slow => "SLOW",
            SporeCloud => "SPOR",
            Split => "SPLT",
            Stasis => "STAS",
            StrengthUp => "STR+",
            TimeWarp => "TIME",
            Asleep => "SLP",
            Duplication => "DUPL",
            LoseStrength => "LSTR",
            LoseDexterity => "LDEX",
            Focus => "FOC",
            Poison => "PSN",
            Accuracy => "ACC",
            AfterImage => "AIMG",
            ThousandCuts => "1KCT",
            Envenom => "ENV",
            InfiniteBlades => "IBLD",
            NoxiousFumes => "FUME",
            ToolsOfTheTrade => "TOTT",
            WraithForm => "WRAI",
            Blur => "BLUR",
            PhantasmalKiller => "PHNT",
            Burst => "BRST",
            WellLaidPlans => "WLP",
            Choke => "CHOK",
            CorpseExplosion => "CORP",
            DoubleDamage => "DBLD",
        }
    }

    /// 负面状态(渲染成红色、会被"清除减益"清掉)
    pub fn is_debuff(self) -> bool {
        use Status::*;
        matches!(
            self,
            Vulnerable
                | Weak
                | Frail
                | Entangled
                | Confused
                | Hex
                | DrawReduction
                | Surrounded
                | Constricted
                | Slow
                | Poison
                | NoDraw
        )
    }

    /// 回合结束时层数减一(持续整场战斗的减益不算)
    pub fn decays(self) -> bool {
        use Status::*;
        if matches!(self, Confused | Hex | Surrounded | Constricted | Slow | Poison) {
            return false;
        }
        // 残影术/幻影杀手是"自己数回合"的(在 combat.rs 的回合开始/结束处减层),
        // 放进这里会被一轮结束时的减益递减顺手扣掉,根本轮不到它生效
        self.is_debuff()
            || matches!(
                self,
                DoubleTap | NoBlock | Duplication | Burst | DoubleDamage
            )
    }

    /// 层数可以降到 0 以下(参考实现里 canGoNegative 的那几条):
    /// 力量/敏捷/集中的减益是把强度减成负数,而不是"减到 0 就消失"
    /// (拉格文的汲魂在 0 力量时会把你压成 -1 力量).
    pub fn can_go_negative(self) -> bool {
        use Status::*;
        matches!(self, Strength | Dexterity | Focus)
    }

}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 一组状态.用 Vec 保存,保证渲染顺序稳定(便于 diff 与测试)
#[derive(Clone, Default, Debug)]
pub struct Statuses {
    list: Vec<(Status, i32)>,
}

impl Statuses {
    pub fn new() -> Self {
        Statuses { list: Vec::new() }
    }

    pub fn get(&self, s: Status) -> i32 {
        self.list
            .iter()
            .find(|(k, _)| *k == s)
            .map(|(_, n)| *n)
            .unwrap_or(0)
    }

    /// 按挂载顺序遍历(原版把加成当 atDamageGive 钩子,按 powers 挂的顺序折叠,
    /// 所以同一条状态先挂后挂会影响结果的第 1 点小数)
    pub fn entries(&self) -> impl Iterator<Item = &(Status, i32)> {
        self.list.iter()
    }

    pub fn has(&self, s: Status) -> bool {
        self.get(s) != 0
    }

    /// 状态栏里挂没挂这一条(层数 0 也算挂着,慢速开局显示的就是 Slow 0)
    pub fn holds(&self, s: Status) -> bool {
        self.list.iter().any(|(k, _)| *k == s)
    }

    /// 挂上一条层数为 0 的状态(用于开局就显示的倒计时/慢速)
    pub fn mark(&mut self, s: Status) {
        if !self.holds(s) {
            self.list.push((s, 0));
        }
    }

    /// 直接把层数设成 n(每回合重置延展/慢速/飞行用);
    /// 0 且不会自然递减的状态留一条空壳,界面上仍然显示(Slow 0)
    pub fn set(&mut self, s: Status, n: i32) {
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            slot.1 = n;
        } else if n > 0 || !s.decays() {
            self.list.push((s, n));
        }
        self.list.retain(|(k, n)| *k != s || *n != 0 || !k.decays());
    }

    /// 叠加 n,n 可以为负;多数状态层数降到 0 就移除,
    /// 力量/敏捷/集中这类"强度"状态允许压到负数(见 can_go_negative).
    /// 它们回到 0 也留着那一条:原版的 ApplyPowerAction 只在 !canGoNegative
    /// 时把 0 层的强度摘掉,留着才能保住它在 powers 列表里的位置(伤害折叠
    /// 按挂载顺序走,重挂会让顺序变、同一场伤害算出来差 1)
    pub fn add(&mut self, s: Status, n: i32) {
        if n == 0 {
            return;
        }
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            slot.1 += n;
            if slot.1 <= 0 && !s.can_go_negative() {
                self.list.retain(|(k, _)| *k != s);
            }
            return;
        }
        if n > 0 || s.can_go_negative() {
            self.list.push((s, n));
        }
    }

    /// 层数翻倍(强化卡与部分药水用),负数不翻
    pub fn double(&mut self, s: Status) {
        if let Some(slot) = self.list.iter_mut().find(|(k, _)| *k == s) {
            if slot.1 > 0 {
                slot.1 *= 2;
            }
        }
    }

    /// 每个到期的状态层数减一
    pub fn decay_debuffs(&mut self) {
        self.decay_debuffs_except(&[]);
    }

    /// 同 decay_debuffs,但 skip 里的状态这一次不减.
    /// 敌人刚给玩家挂上的持续状态(参考实现里的 justApplied)要跳过第一次递减,
    /// 否则持续时间会短一整回合
    pub fn decay_debuffs_except(&mut self, skip: &[Status]) {
        for slot in self.list.iter_mut() {
            if slot.0.decays() && !skip.contains(&slot.0) {
                slot.1 -= 1;
            }
        }
        // 不递减的状态留着(包括层数为 0 的标记,比如 Slow 0)
        self.list.retain(|(k, n)| *n > 0 || !k.decays());
    }

    /// 敌人"清掉自己的减益"用:对齐反编译 Monster::removeDebuffs ——
    /// 先把负的力量归零再清减益(力量是增益,不会被 is_debuff 清掉,所以单靠 retain
    /// 会把 -3 这种负力量留在身上;冠军的怒吼、时间吞噬者的加速都靠这一步回正)
    pub fn clear_debuffs(&mut self) {
        if self.get(Status::Strength) < 0 {
            self.set(Status::Strength, 0);
        }
        self.list.retain(|(k, _)| !k.is_debuff());
    }

    /// 橙皮(Orange Pellets)用:对齐反编译 Player::removeDebuffs 的范围 ——
    /// 比 is_debuff 多清 LoseStrength/LoseDexterity/NoBlock,并把负的力量/敏捷归零
    /// (所以柔韧/力量药水残留的 "Lose Strength" 会被清掉,负力量也回到 0)
    pub fn remove_player_debuffs(&mut self) {
        if self.get(Status::Strength) < 0 {
            self.set(Status::Strength, 0);
        }
        if self.get(Status::Dexterity) < 0 {
            self.set(Status::Dexterity, 0);
        }
        self.list.retain(|(k, _)| {
            !k.is_debuff()
                && !matches!(
                    k,
                    Status::LoseStrength | Status::LoseDexterity | Status::NoBlock
                )
        });
    }

    pub fn iter(&self) -> impl Iterator<Item = (Status, i32)> + '_ {
        self.list.iter().copied()
    }

}

#[cfg(test)]
mod tests {
    /// 图鉴"buff 汇总"靠 Status::ALL 列举:它必须与枚举分支一一对应.
    /// 用 name() 的分支数当基准(那个 match 没有通配分支,新增变体会被编译器逼着补上).
    #[test]
    fn status_all_lists_every_variant() {
        let src = include_str!("status.rs");
        let start = src.find("pub fn name(self)").expect("name() 必须在");
        let tail = &src[start..];
        let end = tail.find("\n        }").expect("name() 的收尾括号必须在");
        let arms = tail[..end].matches(" => \"").count();
        assert_eq!(
            Status::ALL.len(),
            arms,
            "Status::ALL 少列了变体:枚举里有 {arms} 条,name() 与 ALL 必须同步"
        );
        let mut ids: Vec<&str> = Status::ALL.iter().map(|s| s.name()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(n, ids.len(), "Status::ALL 里有重复项");
    }

    /// 图鉴里每条状态都要有一句说明:语料里有就用语料的,没有的走手写兜底
    #[test]
    fn every_status_has_a_summary() {
        for &s in Status::ALL {
            let in_corpus = crate::core::corpus::POWERS.iter().any(|p| p.id == s.power_id());
            assert!(
                in_corpus || !s.summary_fallback().is_empty(),
                "{} 既不在语料 POWERS 里也没有手写兜底",
                s.name()
            );
            assert!(!s.name().is_empty() && !s.short().is_empty());
        }
    }

    use super::*;

    #[test]
    fn add_and_get() {
        let mut s = Statuses::new();
        assert_eq!(s.get(Status::Strength), 0);
        s.add(Status::Strength, 2);
        s.add(Status::Strength, 1);
        assert_eq!(s.get(Status::Strength), 3);
        s.add(Status::Strength, -3);
        assert!(!s.has(Status::Strength));
        // 强度类回到 0 仍留着那一条(保住它在 powers 列表里的位置)
        assert_eq!(s.iter().count(), 1);
    }

    #[test]
    fn negative_add_on_missing_is_noop() {
        let mut s = Statuses::new();
        s.add(Status::Weak, -4);
        assert_eq!(s.iter().count(), 0);
    }

    /// 力量/敏捷可以压到 0 以下(拉格文的汲魂在 0 力量时把你压成 -1);
    /// 回到 0 也留着那一条(原版 canGoNegative 的强度不在 0 层被摘掉,
    /// 位置保住后伤害折叠顺序才对)
    #[test]
    fn strength_can_go_negative() {
        let mut s = Statuses::new();
        s.add(Status::Strength, -1);
        assert_eq!(s.get(Status::Strength), -1);
        s.add(Status::Dexterity, -2);
        assert_eq!(s.get(Status::Dexterity), -2);
        // 负的加上正的回到 0:层数是 0,但条目还在
        s.add(Status::Strength, 1);
        assert!(!s.has(Status::Strength));
        assert!(s.holds(Status::Strength));
        // 减益类的状态仍旧不允许为负
        s.add(Status::Vulnerable, 2);
        s.add(Status::Vulnerable, -5);
        assert!(!s.holds(Status::Vulnerable));
    }

    #[test]
    fn double_only_positive() {
        let mut s = Statuses::new();
        s.add(Status::Strength, 3);
        s.double(Status::Strength);
        assert_eq!(s.get(Status::Strength), 6);
        s.double(Status::DemonForm);
        assert_eq!(s.get(Status::DemonForm), 0);
    }

    #[test]
    fn decay_only_hits_debuffs() {
        let mut s = Statuses::new();
        s.add(Status::Vulnerable, 2);
        s.add(Status::Weak, 1);
        s.add(Status::Strength, 4);
        s.add(Status::DemonForm, 1);
        s.decay_debuffs();
        assert_eq!(s.get(Status::Vulnerable), 1);
        assert!(!s.has(Status::Weak));
        assert_eq!(s.get(Status::Strength), 4);
        assert_eq!(s.get(Status::DemonForm), 1);
    }

    #[test]
    fn clear_debuffs_keeps_powers() {
        let mut s = Statuses::new();
        s.add(Status::Vulnerable, 3);
        s.add(Status::Frail, 2);
        s.add(Status::Metallicize, 3);
        s.clear_debuffs();
        assert_eq!(s.get(Status::Metallicize), 3);
        assert_eq!(s.get(Status::Vulnerable), 0);
    }

    #[test]
    fn iter_order_is_insertion_order() {
        let mut s = Statuses::new();
        s.add(Status::Weak, 1);
        s.add(Status::Strength, 2);
        let v: Vec<_> = s.iter().collect();
        assert_eq!(v, vec![(Status::Weak, 1), (Status::Strength, 2)]);
    }
    /// 图鉴的图标:每条都要有,而且不重样(重了说明挑图标时抄漏了一行)
    #[test]
    fn every_status_has_a_distinct_glyph() {
        let mut seen: Vec<(char, &'static str)> = Vec::new();
        for &s in Status::ALL {
            let g = s.glyph();
            assert!(g as u32 > 0xE000, "{} 没有图标", s.name());
            if let Some((_, other)) = seen.iter().find(|(c, _)| *c == g) {
                panic!("{} 与 {} 共用同一个图标 {:?}", s.name(), other, g);
            }
            seen.push((g, s.name()));
        }
        assert_eq!(seen.len(), Status::ALL.len());
    }

    /// 图标配色键必须是 theme::icon_color 认得的那些(不认得就退回普通前景色,
    /// 这里直接判失败,免得"看着像上了色其实是灰的")
    #[test]
    fn icon_color_keys_are_supported() {
        const KEYS: &[&str] = &[
            "good", "bad", "block", "fire", "poison", "energy", "ghost", "arcane", "blood", "dim",
            "plain",
        ];
        for &s in Status::ALL {
            let k = s.icon_color_key();
            assert!(KEYS.contains(&k), "{} 的配色键 {k} 不在支持列表里", s.name());
        }
    }

    /// 候选图标清单:1~10 个,第一个必须是当前用的那个,而且都不重样
    #[test]
    fn glyph_candidates_are_bounded_and_start_with_the_pick() {
        for &s in Status::ALL {
            let c = s.glyph_candidates();
            assert!(!c.is_empty() && c.len() <= 10, "{} 的候选数 {}", s.name(), c.len());
            assert_eq!(c[0].0, s.glyph(), "{} 的第一个候选要等于当前图标", s.name());
            let mut seen: Vec<char> = Vec::new();
            for (ch, name) in c {
                assert!(!name.is_empty(), "{} 的候选没有字形名", s.name());
                assert!(!seen.contains(ch), "{} 的候选 {:?} 重复", s.name(), ch);
                seen.push(*ch);
            }
        }
    }

}
