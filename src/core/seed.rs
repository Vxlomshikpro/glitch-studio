//! Детерминированный ГСЧ. Один seed — один и тот же глитч, всегда.

use rand_chacha::rand_core::SeedableRng;
use rand_chacha::rand_core::Rng as _;
use rand_chacha::ChaCha8Rng;

/// Обёртка над ChaCha8: детерминирован по конструкции, не зависит от платформы.
pub struct Rng {
    inner: ChaCha8Rng,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            inner: ChaCha8Rng::seed_from_u64(seed),
        }
    }

    /// Подсед для конкретного эффекта: у каждого шага свой поток,
    /// поэтому включение второго эффекта не сдвигает случайность первого.
    pub fn subseed(&mut self, tag: u64) -> Rng {
        let extra = self.inner.next_u64();
        let s =
            self.inner.next_u64() ^ extra.rotate_left(17) ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Rng::new(s)
    }

    /// Дочерний поток с тегом. Детерминирован при том же seed.
    pub fn next_u64_for_tag(&mut self, tag: u64) -> u64 {
        self.inner.next_u64() ^ tag.wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// u32 в диапазоне `[0, n)`. При `n == 0` — 0.
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.inner.next_u32() % n
        }
    }

    /// Непрерывное значение в `[0, 1)`.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.inner.next_u32() >> 8) as f32 / (1 << 24) as f32
    }

    /// Значение в `[lo, hi)`.
    #[inline]
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.unit() * (hi - lo)
    }

    /// Целое в `[lo, hi]` включительно.
    #[inline]
    pub fn range_i(&mut self, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + self.below((hi - lo + 1) as u32) as i64
    }

    /// Вероятность `p` (0..=1).
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}
