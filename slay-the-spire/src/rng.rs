// 确定性伪随机:xoshiro256** + splitmix64 播种.
// 不引入 rand 依赖,同一 seed 永远产生同一条时间线,便于复现一局与写单测.
pub struct Rng {
    s: [u64; 4],
}

fn splitmix64(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut z = seed ^ 0xA076_1D64_78BD_642F;
        let mut s = [0u64; 4];
        for slot in s.iter_mut() {
            *slot = splitmix64(&mut z);
        }
        // 全零状态会让 xoshiro 卡死,换成非零常量
        if s == [0, 0, 0, 0] {
            s[0] = 0x9E37_79B9_7F4A_7C15;
        }
        Rng { s }
    }

    /// 存档用:取当前内部状态
    pub fn state(&self) -> [u64; 4] {
        self.s
    }

    /// 存档用:恢复内部状态
    pub fn set_state(&mut self, s: [u64; 4]) {
        self.s = s;
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// 0..n 的均匀整数,n 为 0 时返回 0
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // 拒绝采样,避免取模偏置
        let limit = u32::MAX - (u32::MAX % n) - 1;
        loop {
            let v = (self.next_u64() >> 32) as u32;
            if v <= limit || limit == u32::MAX {
                return v % n;
            }
        }
    }

    /// [lo, hi] 闭区间
    pub fn range_inclusive(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i32
    }

    /// 百分比命中
    pub fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct.min(100)
    }

    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        if v.len() < 2 {
            return;
        }
        for i in (1..v.len()).rev() {
            let j = self.below((i + 1) as u32) as usize;
            v.swap(i, j);
        }
    }

    /// 从切片里随机取一个,调用方保证非空
    pub fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len() as u32) as usize]
    }

    /// 按权重随机取一个下标,权重全为 0 时返回 None
    pub fn weighted_idx(&mut self, weights: &[u32]) -> Option<usize> {
        let total: u32 = weights.iter().sum();
        if total == 0 {
            return None;
        }
        let mut roll = self.below(total);
        for (i, w) in weights.iter().enumerate() {
            if roll < *w {
                return Some(i);
            }
            roll -= *w;
        }
        weights.iter().rposition(|w| *w > 0)
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        let mut c = Rng::new(12346);
        let sa: Vec<u64> = (0..64).map(|_| a.next_u64()).collect();
        let sb: Vec<u64> = (0..64).map(|_| b.next_u64()).collect();
        let sc: Vec<u64> = (0..64).map(|_| c.next_u64()).collect();
        assert_eq!(sa, sb);
        assert_ne!(sa, sc);
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
    fn shuffle_is_permutation() {
        let mut r = Rng::new(2024);
        let mut v: Vec<i32> = (0..50).collect();
        r.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort();
        assert_eq!(sorted, (0..50).collect::<Vec<i32>>());
        assert_ne!(v, sorted);
    }

    #[test]
    fn shuffle_of_one_and_empty_is_safe() {
        let mut r = Rng::new(1);
        let mut one = [42];
        r.shuffle(&mut one);
        assert_eq!(one, [42]);
        let mut empty: [i32; 0] = [];
        r.shuffle(&mut empty);
    }

    #[test]
    fn weighted_idx_respects_zero_weights() {
        let mut r = Rng::new(5);
        assert_eq!(r.weighted_idx(&[0, 0]), None);
        assert_eq!(r.weighted_idx(&[]), None);
        for _ in 0..200 {
            assert_eq!(r.weighted_idx(&[0, 3, 0]), Some(1));
        }
        // 只出现权重非零的下标
        let mut seen = [false; 3];
        for _ in 0..2000 {
            if let Some(i) = r.weighted_idx(&[1, 0, 5]) {
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

}
