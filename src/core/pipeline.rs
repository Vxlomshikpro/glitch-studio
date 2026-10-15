//! Стек эффектов: порядок, включение, Mix, seed, chaos.
//!
//! Пайплайн — чистая функция: на вход кадр (+ необязательная история кадров),
//! на выход новый кадр. Никакого состояния внутри, кроме заданного снаружи.

use serde::{Deserialize, Serialize};

use crate::core::effect::EffectCtx;
use crate::core::error::Result;
use crate::core::frame::Frame;
use crate::core::params::ParamValues;
use crate::core::registry::Registry;
use crate::core::seed::Rng;

/// Одна запись стека: какой эффект, включён ли, с каким миксом и значениями.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StackEntry {
    pub id: String,
    pub enabled: bool,
    /// 0..=1 — доля эффекта в результате.
    pub mix: f32,
    pub values: ParamValues,
}

impl StackEntry {
    pub fn new(id: &str, values: ParamValues) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            mix: 1.0,
            values,
        }
    }
}

/// Полный стек эффектов.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pipeline {
    pub seed: u64,
    /// 0..=100. Глобальный множитель интенсивности всех эффектов.
    pub chaos: f32,
    pub stack: Vec<StackEntry>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            seed: 42,
            chaos: 100.0,
            stack: Vec::new(),
        }
    }
}

impl Pipeline {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            ..Default::default()
        }
    }

    /// Пустой пайплайн — все шаги отключены.
    pub fn with_defaults(seed: u64, registry: &Registry) -> Self {
        let mut p = Self::new(seed);
        for eff in registry.all() {
            p.stack.push(StackEntry::new(eff.id(), eff.defaults()));
        }
        p
    }

    pub fn chaos_norm(&self) -> f32 {
        (self.chaos / 100.0).clamp(0.0, 1.0)
    }

    pub fn is_empty(&self) -> bool {
        self.stack.iter().all(|e| !e.enabled || e.mix <= 0.0)
    }

    /// Заменяет набор шагов на дефолтный по реестру, сохраняя seed.
    pub fn reset_to_defaults(&mut self, registry: &Registry) {
        for entry in &mut self.stack {
            if let Ok(eff) = registry.get(&entry.id) {
                entry.values = eff.defaults();
                entry.mix = 1.0;
                entry.enabled = true;
            }
        }
    }

    /// Подгоняет значения под текущие спецификации (после смены версии/состава).
    pub fn reconcile(&mut self, registry: &Registry) {
        self.chaos = self.chaos.clamp(0.0, 100.0);
        for entry in &mut self.stack {
            if let Ok(eff) = registry.get(&entry.id) {
                entry.mix = entry.mix.clamp(0.0, 1.0);
                entry.values.reconcile(eff.params());
            }
        }
    }

    /// Перемещает шаг с `from` на `to` — для drag & drop в UI.
    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.stack.len() || to >= self.stack.len() || from == to {
            return;
        }
        let item = self.stack.remove(from);
        self.stack.insert(to, item);
    }

    /// Хэш содержимого для кеша превью. `f32` хэшируется через биты,
    /// чтобы 0.1 и 0.100000001 не давали разный хэш.
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.seed.hash(&mut h);
        self.chaos.to_bits().hash(&mut h);
        for e in &self.stack {
            e.id.hash(&mut h);
            e.enabled.hash(&mut h);
            e.mix.to_bits().hash(&mut h);
            for v in &e.values.values {
                v.to_bits().hash(&mut h);
            }
        }
        h.finish()
    }

    /// Применяет стек к кадру.
    ///
    /// Каждый шаг считается в отдельном буфере и смешивается с исходником по `mix`
    /// на уровне пикселей — это даёт предсказуемый «приправочный» эффект.
    /// Ошибка шага не роняет пайплайн: она возвращается, а кадр остаётся пригодным.
    pub fn apply(
        &self,
        input: &Frame,
        registry: &Registry,
        prev: &[std::sync::Arc<Frame>],
    ) -> Result<Frame> {
        let mut current = input.clone();
        let chaos = self.chaos_norm();

        for (idx, entry) in self.stack.iter().enumerate() {
            if !entry.enabled || entry.mix <= 0.0 {
                continue;
            }
            let eff = match registry.get(&entry.id) {
                Ok(e) => e,
                // Неизвестный id в старом пресете — пропускаем, а не падаем.
                Err(_) => continue,
            };

            let mut scratch = current.clone();
            // Подсед стабилен: seed + позиция в стеке + id эффекта.
            let mut rng = {
                let mut base = Rng::new(self.seed);
                let mut s = base.subseed(idx as u64);
                let mut h: u64 = 0xcbf2_9ce4_8422_2325;
                for b in eff.id().as_bytes() {
                    h ^= *b as u64;
                    h = h.wrapping_mul(0x1000_0000_01b3);
                }
                s = Rng::new(s.next_u64_for_tag(h));
                s
            };

            let mut ctx = EffectCtx {
                frame: &mut scratch,
                values: &entry.values,
                rng: &mut rng,
                chaos,
                prev,
            };

            eff.apply(&mut ctx)?;

            let mix = entry.mix.clamp(0.0, 1.0);
            if mix >= 1.0 {
                current = scratch;
            } else {
                current.blend_from(&scratch, mix);
            }
        }
        Ok(current)
    }
}
