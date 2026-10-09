// 随机数:照抄参考实现(refs/slay-the-cli 的 src/engine/core/rng.ts)的那套栈.
//
// - `Rng` 是 libGDX 的 RandomXS128(xorshift128+),种子先过 murmurhash3 混淆,
//   每次公开掷点都会把内部 call counter 加一(存档靠 counter 对齐).
// - `JavaRandom` 是 java.util.Random 的 48 位 LCG,牌堆洗牌用它.
// - 种子对外是 base-35 字符串(字母表不含字母 O),与 u64 双向转换.
// - `RngRegistry` 按用途把随机分成具名流:跑一局的流(run)开局定种、整局共用;
//   每层重开的流(floor)在每次进房间时用 seed+层号重种;地图按章节重种.
//
// 全仓的取数点都从具名流取,不再有单独一条全局流.

/// 随机种子:时间纳秒混上进程号,不用第三方的 rand
pub fn random_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u64) << 32
}

const MURMUR_C1: u64 = 0xff51_afd7_ed55_8ccd;
const MURMUR_C2: u64 = 0xc4ce_b9fe_1a85_ec53;
const ONE_IN_MOST_SIGNIFICANT: u64 = 1 << 63;

fn murmur_hash3(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(MURMUR_C1);
    x ^= x >> 33;
    x = x.wrapping_mul(MURMUR_C2);
    x ^= x >> 33;
    x
}

/// 一条流的存档快照:两个内部字 + 计数器
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RngState {
    pub seed0: u64,
    pub seed1: u64,
    pub counter: u32,
}

/// RandomXS128(xorshift128+):参考实现里的 sts.Random
#[derive(Clone, Debug)]
pub struct Rng {
    seed0: u64,
    seed1: u64,
    counter: u32,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let s = seed;
        let seed0 = murmur_hash3(if s == 0 { ONE_IN_MOST_SIGNIFICANT } else { s });
        let seed1 = murmur_hash3(seed0);
        Rng {
            seed0,
            seed1,
            counter: 0,
        }
    }

    /// 存档恢复时参考实现的做法:空烧 target 次 random(999) 把计数器推上去
    #[cfg(test)]
    pub fn with_counter(seed: u64, target: u32) -> Self {
        let mut r = Rng::new(seed);
        for _ in 0..target {
            r.random(999);
        }
        r
    }

    pub fn state(&self) -> RngState {
        RngState {
            seed0: self.seed0,
            seed1: self.seed1,
            counter: self.counter,
        }
    }

    pub fn from_state(s: RngState) -> Self {
        Rng {
            seed0: s.seed0,
            seed1: s.seed1,
            counter: s.counter,
        }
    }

    // ---- 原始发生器:不动计数器 ----

    fn next_long(&mut self) -> u64 {
        let mut s1 = self.seed0;
        let s0 = self.seed1;
        self.seed0 = s0;
        s1 ^= s1 << 23;
        self.seed1 = s1 ^ s0 ^ (s1 >> 17) ^ (s0 >> 26);
        self.seed1.wrapping_add(s0)
    }

    /// 拒绝采样的有界 nextLong,n > 0
    fn next_long_bounded(&mut self, n: u64) -> u64 {
        loop {
            let bits = self.next_long() >> 1;
            let value = bits % n;
            if (bits.wrapping_sub(value).wrapping_add(n).wrapping_sub(1) as i64) >= 0 {
                return value;
            }
        }
    }

    fn next_float(&mut self) -> f32 {
        (self.next_long() >> 40) as f32 * 5.960_464_477_539_062_5e-8
    }

    // ---- 公开掷点:每次调用都把计数器加一 ----

    /// [0, range] 闭区间(参考实现的 Random.random(int))
    pub fn random(&mut self, range: u32) -> u32 {
        self.counter += 1;
        self.next_long_bounded(range as u64 + 1) as u32
    }

    /// [start, end] 闭区间
    pub fn random_range(&mut self, start: i32, end: i32) -> i32 {
        self.counter += 1;
        start + self.next_long_bounded((end - start + 1) as u64) as i32
    }

    /// [0, 1) 的 float32
    pub fn random_float(&mut self) -> f32 {
        self.counter += 1;
        self.next_float()
    }

    /// [start, end) 的 float32
    pub fn random_float_range(&mut self, start: f32, end: f32) -> f32 {
        self.counter += 1;
        start + self.next_float() * (end - start)
    }

    pub fn random_long(&mut self) -> u64 {
        self.counter += 1;
        self.next_long()
    }

    /// 概率命中:chance 是 0..1 的 float32
    pub fn random_bool_chance(&mut self, chance: f32) -> bool {
        self.counter += 1;
        self.next_float() < chance
    }

    /// 随机布尔:拿 nextLong 的最低位(参考实现的 randomBoolean())
    pub fn random_boolean(&mut self) -> bool {
        self.counter += 1;
        self.next_long() & 1 != 0
    }

    /// 计数器当前值
    pub fn counter(&self) -> u32 {
        self.counter
    }

    /// 把计数器推到 target:一路烧 randomBoolean()(参考实现的 setCounter)
    pub fn set_counter(&mut self, target: u32) {
        while self.counter < target {
            self.random_boolean();
        }
    }

    /// 不计数器的有界整数:地图生成器内部那圈洗牌直接调它
    pub fn next_int_raw(&mut self, n: u32) -> u32 {
        self.next_long_bounded(n as u64) as u32
    }

    // ---- 本仓库代码在用的几个顺手的封装,语义都按上面那套 ----

    /// [lo, hi] 闭区间
    pub fn range_inclusive(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        self.random_range(lo, hi)
    }

    /// [0, n) 的整数;n 为 0 时返回 0(不消耗掷点)
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.random(n - 1)
        }
    }

    /// 从切片里随机取一个,调用方保证非空
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.random(v.len() as u32 - 1) as usize]
    }

    /// 按 float32 权重累积走一遍挑下标(参考实现的 rollWeightedIdx);
    /// 权重全为 0 时返回 None
    pub fn weighted_idx_f32(&mut self, weights: &[f32]) -> Option<usize> {
        if weights.is_empty() {
            return None;
        }
        let roll = self.random_float();
        let mut cur = 0.0f32;
        for (i, w) in weights.iter().enumerate() {
            cur += *w;
            if roll < cur {
                return Some(i);
            }
        }
        if weights.iter().any(|w| *w > 0.0) {
            Some(weights.len() - 1)
        } else {
            None
        }
    }
}

// ---- java.util.Random(48 位 LCG)+ Collections.shuffle ----

const J_MULT: u64 = 0x5deece66d;
const J_ADD: u64 = 0xb;
const J_MASK: u64 = (1 << 48) - 1;

/// 洗牌用的 java.util.Random
#[derive(Clone, Debug)]
pub struct JavaRandom {
    seed: u64,
}

impl JavaRandom {
    pub fn new(seed: u64) -> Self {
        JavaRandom {
            seed: (seed ^ J_MULT) & J_MASK,
        }
    }

    fn next(&mut self, bits: u32) -> u32 {
        self.seed = (self.seed.wrapping_mul(J_MULT).wrapping_add(J_ADD)) & J_MASK;
        (self.seed >> (48 - bits)) as u32
    }

    pub fn next_int(&mut self, bound: u32) -> u32 {
        let mut r = self.next(31);
        let m = bound - 1;
        if bound & m == 0 {
            ((bound as u64 * r as u64) >> 31) as u32
        } else {
            let mut u = r;
            loop {
                r = u % bound;
                if ((u.wrapping_sub(r).wrapping_add(m)) as i32) >= 0 {
                    return r;
                }
                u = self.next(31);
            }
        }
    }
}

/// 参考实现里的 Collections.shuffle(原地)
pub fn java_shuffle<T>(arr: &mut [T], rnd: &mut JavaRandom) {
    let mut i = arr.len();
    while i > 1 {
        let j = rnd.next_int(i as u32) as usize;
        arr.swap(i - 1, j);
        i -= 1;
    }
}

// ---- 种子字符串:base 35,字母表不含 O ----

const SEED_BASE: u64 = 35;
const SEED_CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNPQRSTUVWXYZ";

/// u64 种子转参考实现那种 base-35 种子串
pub fn seed_to_string(seed: u64) -> String {
    let mut u = seed;
    let mut out: Vec<u8> = Vec::new();
    loop {
        let rem = (u % SEED_BASE) as usize;
        u /= SEED_BASE;
        out.push(SEED_CHARS[rem]);
        if u == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).expect("base35 字母表都是 ascii")
}

/// base-35 种子串转 u64;字母表外的字符按参考实现那样折算
pub fn seed_from_string(seed: &str) -> u64 {
    let mut ret: u64 = 0;
    for raw in seed.chars() {
        let c = raw.to_ascii_uppercase() as u32;
        let value = if c < 65 {
            c.wrapping_sub(48)
        } else if c < 79 {
            c - 65 + 10
        } else {
            c - 65 + 9
        };
        ret = ret.wrapping_mul(SEED_BASE).wrapping_add(value as u64);
    }
    ret
}

/// 命令行给的种子串:纯十进制就按数字用,否则按 base-35 种子串解释
pub fn seed_from_arg(arg: &str) -> Option<u64> {
    let t = arg.trim();
    if t.is_empty() {
        return None;
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return t.parse::<u64>().ok();
    }
    if t.chars()
        .all(|c| c.is_ascii_alphanumeric() && c.to_ascii_uppercase() != 'O')
    {
        return Some(seed_from_string(t));
    }
    None
}

// ---- 具名流 ----

/// 整局用一条的流
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunStream {
    /// 卡牌奖励 + 稀有度保底
    CardRng,
    /// 事件池抽取
    EventRng,
    /// 商店
    MerchantRng,
    /// 遭遇表生成
    MonsterRng,
    /// Neow 祝福
    NeowRng,
    /// 药水掉落
    PotionRng,
    /// 遗物掉落与遗物池洗牌
    RelicRng,
    /// 金币与宝箱
    TreasureRng,
}

/// 每个房间重新种一次的流
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloorStream {
    /// 怪物选招
    AiRng,
    /// 战斗内的随机:牌堆位置、随机目标、随机卡
    CardRandomRng,
    /// 战斗与事件里的杂项掷点
    MiscRng,
    /// 怪物血量
    MonsterHpRng,
    /// 洗牌(只用来喂 JavaRandom)
    ShuffleRng,
}

pub const RUN_STREAMS: [RunStream; 8] = [
    RunStream::CardRng,
    RunStream::EventRng,
    RunStream::MerchantRng,
    RunStream::MonsterRng,
    RunStream::NeowRng,
    RunStream::PotionRng,
    RunStream::RelicRng,
    RunStream::TreasureRng,
];

pub const FLOOR_STREAMS: [FloorStream; 5] = [
    FloorStream::AiRng,
    FloorStream::CardRandomRng,
    FloorStream::MiscRng,
    FloorStream::MonsterHpRng,
    FloorStream::ShuffleRng,
];

impl RunStream {
    pub fn name(self) -> &'static str {
        match self {
            RunStream::CardRng => "cardRng",
            RunStream::EventRng => "eventRng",
            RunStream::MerchantRng => "merchantRng",
            RunStream::MonsterRng => "monsterRng",
            RunStream::NeowRng => "neowRng",
            RunStream::PotionRng => "potionRng",
            RunStream::RelicRng => "relicRng",
            RunStream::TreasureRng => "treasureRng",
        }
    }

    fn from_name(name: &str) -> Option<RunStream> {
        RUN_STREAMS.into_iter().find(|s| s.name() == name)
    }
}

impl FloorStream {
    pub fn name(self) -> &'static str {
        match self {
            FloorStream::AiRng => "aiRng",
            FloorStream::CardRandomRng => "cardRandomRng",
            FloorStream::MiscRng => "miscRng",
            FloorStream::MonsterHpRng => "monsterHpRng",
            FloorStream::ShuffleRng => "shuffleRng",
        }
    }

    fn from_name(name: &str) -> Option<FloorStream> {
        FLOOR_STREAMS.into_iter().find(|s| s.name() == name)
    }
}

const MAP_STREAM: &str = "mapRng";
const MATH_UTIL_STREAM: &str = "mathUtilRng";

/// 具名流的集合:所有取数点都从这里拿流
#[derive(Clone, Debug)]
pub struct RngRegistry {
    seed: u64,
    run: Vec<Rng>,
    floor: Vec<Rng>,
    map: Rng,
    math_util: Rng,
}

impl RngRegistry {
    /// 开局:各跑一局的流都以 run seed 定种;每层的流先用 seed+0;
    /// 地图用第一章的偏移(seed+1)
    pub fn new(seed: u64) -> Self {
        RngRegistry {
            seed,
            run: RUN_STREAMS.iter().map(|_| Rng::new(seed)).collect(),
            floor: FLOOR_STREAMS.iter().map(|_| Rng::new(seed)).collect(),
            map: Rng::new(seed),
            math_util: Rng::new(seed.wrapping_sub(897_897)),
        }
        .reseeded(0, 1)
    }

    fn reseeded(mut self, floor: u32, act: u32) -> Self {
        self.reseed_floor_streams(floor);
        self.reseed_map(act);
        self
    }

    pub fn run(&mut self, s: RunStream) -> &mut Rng {
        let i = RUN_STREAMS.iter().position(|x| *x == s).expect("流在表里");
        &mut self.run[i]
    }

    pub fn floor(&mut self, s: FloorStream) -> &mut Rng {
        let i = FLOOR_STREAMS.iter().position(|x| *x == s).expect("流在表里");
        &mut self.floor[i]
    }

    pub fn map_rng(&mut self) -> &mut Rng {
        &mut self.map
    }

    /// 数学工具流(参考实现的 mathUtilRng):商店信使补货抽职业牌这类杂项取数点
    pub fn math_util(&mut self) -> &mut Rng {
        &mut self.math_util
    }

    /// 进房间时调用:每层的流都用 seed+层号重开
    pub fn reseed_floor_streams(&mut self, floor: u32) {
        let s = self.seed.wrapping_add(floor as u64);
        for slot in self.floor.iter_mut() {
            *slot = Rng::new(s);
        }
    }

    /// 开新一章时调用:第一章 seed+1,后面 act*100*(act-1)
    pub fn reseed_map(&mut self, act: u32) {
        let offset = if act == 1 {
            1
        } else {
            (act as u64).wrapping_mul(100).wrapping_mul(act as u64 - 1)
        };
        self.map = Rng::new(self.seed.wrapping_add(offset));
    }

    /// 存档:每条流一行`名字=seed0,seed1,counter`
    pub fn save_text(&self) -> String {
        let mut out = String::new();
        let mut push = |name: &str, r: &Rng| {
            let s = r.state();
            out.push_str(&format!(
                "{name}={},{},{}\n",
                s.seed0, s.seed1, s.counter
            ));
        };
        for (i, name) in RUN_STREAMS.iter().enumerate() {
            push(name.name(), &self.run[i]);
        }
        for (i, name) in FLOOR_STREAMS.iter().enumerate() {
            push(name.name(), &self.floor[i]);
        }
        push(MAP_STREAM, &self.map);
        push(MATH_UTIL_STREAM, &self.math_util);
        out
    }

    /// 存档恢复:必须 15 条流一条不缺,少一条就当成坏档
    pub fn load_line(&mut self, key: &str, value: &str) -> Result<(), String> {
        let parts: Vec<&str> = value.split(',').collect();
        if parts.len() != 3 {
            return Err(format!("存档里的 {key} 状态坏了"));
        }
        let nums: Result<Vec<u64>, _> = parts.iter().map(|p| p.trim().parse::<u64>()).collect();
        let nums = nums.map_err(|_| format!("存档里的 {key} 状态坏了"))?;
        let counter: u32 = parts[2]
            .trim()
            .parse()
            .map_err(|_| format!("存档里的 {key} 计数器坏了"))?;
        let state = RngState {
            seed0: nums[0],
            seed1: nums[1],
            counter,
        };
        if let Some(s) = RunStream::from_name(key) {
            let i = RUN_STREAMS.iter().position(|x| *x == s).unwrap();
            self.run[i] = Rng::from_state(state);
            return Ok(());
        }
        if let Some(s) = FloorStream::from_name(key) {
            let i = FLOOR_STREAMS.iter().position(|x| *x == s).unwrap();
            self.floor[i] = Rng::from_state(state);
            return Ok(());
        }
        if key == MAP_STREAM {
            self.map = Rng::from_state(state);
            return Ok(());
        }
        if key == MATH_UTIL_STREAM {
            self.math_util = Rng::from_state(state);
            return Ok(());
        }
        Err(format!("存档里有一条不认识的随机流 {key}"))
    }

    /// 存档里是不是一条流都没写(老存档)
    pub fn has_any_stream_line(text: &str) -> bool {
        RUN_STREAMS
            .iter()
            .map(|s| s.name())
            .chain(FLOOR_STREAMS.iter().map(|s| s.name()))
            .chain([MAP_STREAM, MATH_UTIL_STREAM])
            .any(|name| text.lines().any(|l| l.starts_with(&format!("{name}="))))
    }
}

/// 存档里的这个键是不是一条具名流
pub fn is_stream_key(name: &str) -> bool {
    RunStream::from_name(name).is_some()
        || FloorStream::from_name(name).is_some()
        || name == MAP_STREAM
        || name == MATH_UTIL_STREAM
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考实现同一套参数下的原位输出(由 bun 直接跑 refs 的 rng.ts 得到)
    #[test]
    fn matches_reference_stream_values() {
        let mut r = Rng::new(0);
        let got: Vec<u64> = (0..5).map(|_| r.random_long()).collect();
        assert_eq!(
            got,
            vec![
                2940871956904845945,
                16801301263782117921,
                17556626904023331505,
                14283134097415469984,
                7191125066339934462,
            ]
        );

        let mut r = Rng::new(0);
        let got: Vec<u32> = (0..5).map(|_| r.random(99)).collect();
        assert_eq!(got, vec![72, 60, 52, 92, 31]);

        let mut r = Rng::new(0);
        let got: Vec<i32> = (0..4).map(|_| r.random_range(5, 7)).collect();
        assert_eq!(got, vec![6, 6, 5, 6]);
    }

    #[test]
    fn java_random_matches_reference() {
        let mut j = JavaRandom::new(12345);
        let got: Vec<u32> = (0..6).map(|_| j.next_int(10)).collect();
        assert_eq!(got, vec![1, 0, 1, 8, 5, 4]);
        let mut v: Vec<u32> = (0..7).collect();
        java_shuffle(&mut v, &mut JavaRandom::new(0));
        assert_eq!(v, vec![1, 0, 3, 2, 6, 4, 5]);
    }

    #[test]
    fn seed_string_round_trips() {
        assert_eq!(seed_to_string(0), "0");
        assert_eq!(seed_to_string(1), "1");
        assert_eq!(seed_to_string(35), "10");
        assert_eq!(seed_to_string(77), "27");
        for s in [0u64, 1, 34, 35, 77, 12345, u64::MAX] {
            assert_eq!(seed_from_string(&seed_to_string(s)), s);
        }
        assert_eq!(seed_from_string("SPIRE"), 41568849);
        assert_eq!(seed_to_string(seed_from_string("SPIRE")), "SPIRE");
        // 命令行:纯十进制按数字用,其余按 base-35 种子串读
        assert_eq!(seed_from_arg("77"), Some(77));
        assert_eq!(seed_from_arg("A2Q"), Some(12345));
        assert_eq!(seed_from_arg("SPIRE"), Some(41568849));
        assert_eq!(seed_from_arg(""), None);
        assert_eq!(seed_from_arg("oops!"), None);
    }

    #[test]
    fn counter_ticks_on_every_public_call() {
        let mut r = Rng::new(7);
        assert_eq!(r.state().counter, 0);
        r.random(9);
        r.random_range(1, 4);
        r.random_float();
        r.random_bool_chance(0.5);
        assert_eq!(r.state().counter, 4);
        // 不计数器的原始掷点不动计数器
        r.next_int_raw(5);
        assert_eq!(r.state().counter, 4);
    }

    #[test]
    fn with_counter_replays_like_the_game() {
        let mut a = Rng::new(1234);
        for _ in 0..30 {
            a.random(999);
        }
        let b = Rng::with_counter(1234, 30);
        assert_eq!(a.state(), b.state());
    }

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        let mut c = Rng::new(12346);
        let sa: Vec<u64> = (0..64).map(|_| a.random_long()).collect();
        let sb: Vec<u64> = (0..64).map(|_| b.random_long()).collect();
        let sc: Vec<u64> = (0..64).map(|_| c.random_long()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
    }

    #[test]
    fn state_round_trips() {
        let mut a = Rng::new(99);
        for _ in 0..17 {
            a.random(50);
        }
        let b = Rng::from_state(a.state());
        let mut b = b;
        assert_eq!(a.random(1000), b.random(1000));
    }

    #[test]
    fn below_stays_in_range() {
        let mut r = Rng::new(7);
        for n in [1u32, 2, 3, 7, 100] {
            for _ in 0..500 {
                assert!(r.below(n) < n);
            }
        }
        assert_eq!(r.below(0), 0);
    }

    #[test]
    fn range_inclusive_bounds() {
        let mut r = Rng::new(99);
        for _ in 0..1000 {
            let v = r.range_inclusive(3, 6);
            assert!((3..=6).contains(&v));
        }
        assert_eq!(r.range_inclusive(5, 5), 5);
        assert_eq!(r.range_inclusive(9, 2), 9);
    }

    #[test]
    fn random_range_matches_naive_bounds() {
        let mut r = Rng::new(4);
        for _ in 0..1000 {
            let v = r.random_range(0, 99);
            assert!((0..=99).contains(&v));
            let v = r.random_range(-5, 5);
            assert!((-5..=5).contains(&v));
        }
    }

    #[test]
    fn java_shuffle_is_permutation() {
        let mut v: Vec<i32> = (0..50).collect();
        java_shuffle(&mut v, &mut JavaRandom::new(2024));
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<i32>>());
        assert_ne!(v, sorted);
    }

    #[test]
    fn java_shuffle_of_one_and_empty_is_safe() {
        let mut one = [42];
        java_shuffle(&mut one, &mut JavaRandom::new(1));
        assert_eq!(one, [42]);
        let mut empty: [i32; 0] = [];
        java_shuffle(&mut empty, &mut JavaRandom::new(1));
    }

    #[test]
    fn weighted_idx_respects_zero_weights() {
        let mut r = Rng::new(5);
        assert_eq!(r.weighted_idx_f32(&[]), None);
        assert_eq!(r.weighted_idx_f32(&[0.0, 0.0]), None);
        for _ in 0..200 {
            assert_eq!(r.weighted_idx_f32(&[0.0, 1.0, 0.0]), Some(1));
        }
        // 权重和小于 1 时,掷点超出累计值会兜底取最后一个下标
        // (参考实现的 rollWeightedIdx 就是这样),所以这两种结果都可能出现
        for _ in 0..200 {
            let got = r.weighted_idx_f32(&[0.0, 0.3, 0.0]);
            assert!(matches!(got, Some(1) | Some(2)), "{got:?}");
        }
        let mut seen = [false; 3];
        for _ in 0..2000 {
            if let Some(i) = r.weighted_idx_f32(&[0.1, 0.0, 0.5]) {
                seen[i] = true;
            }
        }
        assert!(seen[0] && !seen[1] && seen[2]);
    }

    #[test]
    fn below_is_roughly_uniform() {
        let mut r = Rng::new(31337);
        let mut buckets = [0u32; 4];
        for _ in 0..40000 {
            buckets[r.below(4) as usize] += 1;
        }
        for b in buckets {
            assert!(b > 9000 && b < 11000, "bucket {b} 偏离均匀分布太远");
        }
    }

    #[test]
    fn run_streams_are_independent_of_each_other() {
        let mut reg = RngRegistry::new(42);
        let a = reg.run(RunStream::CardRng).random(99);
        let b = reg.run(RunStream::MonsterRng).random(99);
        let mut reg2 = RngRegistry::new(42);
        assert_eq!(reg2.run(RunStream::MonsterRng).random(99), b);
        assert_eq!(reg2.run(RunStream::CardRng).random(99), a);
    }

    #[test]
    fn registry_save_load_round_trips() {
        let mut reg = RngRegistry::new(2024);
        for _ in 0..20 {
            reg.run(RunStream::CardRng).random(99);
            reg.floor(FloorStream::MiscRng).random(99);
            reg.map_rng().random(99);
        }
        let text = reg.save_text();
        let mut back = RngRegistry::new(7);
        for line in text.lines() {
            let (k, v) = line.split_once('=').expect("存档行有等号");
            back.load_line(k, v).expect("存档行能读回来");
        }
        assert_eq!(
            reg.run(RunStream::CardRng).state(),
            back.run(RunStream::CardRng).state()
        );
        assert_eq!(
            reg.floor(FloorStream::MiscRng).state(),
            back.floor(FloorStream::MiscRng).state()
        );
        assert_eq!(reg.map_rng().state(), back.map_rng().state());
    }

    #[test]
    fn floor_streams_reseed_with_floor_number() {
        let mut reg = RngRegistry::new(11);
        reg.reseed_floor_streams(3);
        let mut want = Rng::new(14);
        assert_eq!(
            reg.floor(FloorStream::AiRng).random(99),
            want.random(99)
        );
    }

    #[test]
    fn map_reseed_offsets_follow_the_acts() {
        let mut reg = RngRegistry::new(0);
        reg.reseed_map(2);
        let mut want = Rng::new(200);
        assert_eq!(reg.map_rng().random(99), want.random(99));
        reg.reseed_map(3);
        let mut want = Rng::new(600);
        assert_eq!(reg.map_rng().random(99), want.random(99));
    }

    #[test]
    fn old_save_line_is_rejected_with_a_message() {
        let mut reg = RngRegistry::new(1);
        let err = reg.load_line("cardRng", "1,2,3,4").unwrap_err();
        assert!(err.contains("cardRng"), "{err}");
    }
}
