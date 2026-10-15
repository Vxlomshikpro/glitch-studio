//! Описание параметров эффекта и их значений.
//!
//! Эффект объявляет статический список `ParamSpec`, а экземпляр стека хранит
//! `ParamValues` — вектор `f32` той же длины, индексированный по спецификации.

use serde::{Deserialize, Serialize};

/// Один ползунок.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// Шаг кнопок-стрелок. `0.0` — непрерывное значение.
    pub step: f32,
}

impl ParamSpec {
    pub const fn new(
        key: &'static str,
        label: &'static str,
        min: f32,
        max: f32,
        default: f32,
        step: f32,
    ) -> Self {
        Self {
            key,
            label,
            min,
            max,
            default,
            step,
        }
    }

    pub const fn pct(key: &'static str, label: &'static str, default: f32) -> Self {
        Self::new(key, label, 0.0, 100.0, default, 0.1)
    }

    pub const fn int(
        key: &'static str,
        label: &'static str,
        min: f32,
        max: f32,
        default: f32,
    ) -> Self {
        Self::new(key, label, min, max, default, 1.0)
    }

    /// Форматирование значения для подписи: шаги >= 1 показываем целым.
    pub fn format(&self, v: f32) -> String {
        let v = v.clamp(self.min, self.max);
        if self.step >= 1.0 {
            format!("{}", v.round() as i64)
        } else if self.step == 0.0 {
            format!("{v:.2}")
        } else {
            let decimals = if self.step >= 0.1 {
                1
            } else if self.step >= 0.01 {
                2
            } else {
                3
            };
            format!("{v:.decimals$}")
        }
    }
}

/// Значения всех ползунков одного эффекта.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamValues {
    pub values: Vec<f32>,
}

impl ParamValues {
    /// Собирает значения по умолчанию из спецификации.
    pub fn defaults_from(specs: &[ParamSpec]) -> Self {
        Self {
            values: specs.iter().map(|s| s.default).collect(),
        }
    }

    /// Значение по индексу. Некорректный индекс — 0.0, не паника.
    #[inline]
    pub fn get(&self, idx: usize) -> f32 {
        self.values.get(idx).copied().unwrap_or(0.0)
    }

    #[inline]
    pub fn set(&mut self, idx: usize, v: f32) {
        if let Some(slot) = self.values.get_mut(idx) {
            *slot = v;
        }
    }

    /// Значение по ключу спецификации.
    pub fn by_key(&self, specs: &[ParamSpec], key: &str) -> f32 {
        specs
            .iter()
            .position(|s| s.key == key)
            .map(|i| self.get(i))
            .unwrap_or(0.0)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Подгоняет набор значений под новую спецификацию: недостающие — дефолт,
    /// лишние отбрасываются. Нужно при смене версии пресета.
    pub fn reconcile(&mut self, specs: &[ParamSpec]) {
        let mut out = Vec::with_capacity(specs.len());
        for (i, s) in specs.iter().enumerate() {
            out.push(
                self.values
                    .get(i)
                    .copied()
                    .unwrap_or(s.default)
                    .clamp(s.min, s.max),
            );
        }
        self.values = out;
    }

    /// Все значения зажаты в допустимые диапазоны.
    pub fn clamped(&self, specs: &[ParamSpec]) -> Self {
        let mut out = self.clone();
        out.reconcile(specs);
        out
    }
}
