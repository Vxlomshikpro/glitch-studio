//! Трейт эффекта и контекст его вызова.

use serde::{Deserialize, Serialize};

use crate::core::error::GlitchError;
use crate::core::frame::Frame;
use crate::core::params::ParamValues;
use crate::core::seed::Rng;

/// Группа эффекта — для порядка в UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectGroup {
    Color,
    Geometry,
    Pixels,
    Artifacts,
    Temporal,
}

impl EffectGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "Цвет и каналы",
            Self::Geometry => "Геометрия",
            Self::Pixels => "Сортировка и пиксели",
            Self::Artifacts => "Артефакты и порча данных",
            Self::Temporal => "Видео",
        }
    }

    pub const ALL: [EffectGroup; 5] = [
        Self::Color,
        Self::Geometry,
        Self::Pixels,
        Self::Artifacts,
        Self::Temporal,
    ];
}

/// Всё, что нужно эффекту во время применения.
pub struct EffectCtx<'a> {
    pub frame: &'a mut Frame,
    pub values: &'a ParamValues,
    pub rng: &'a mut Rng,
    /// Глобальный Chaos, 0..=1 (в пайплайне умножается на 100).
    pub chaos: f32,
    /// Предыдущие кадры, самый свежий — последний. Для видео-эффектов.
    /// Для картинок пусто. `Arc`, чтобы не копировать мегабайты на каждый кадр.
    pub prev: &'a [std::sync::Arc<crate::core::frame::Frame>],
}

impl EffectCtx<'_> {
    /// N-ый кадр назад: 0 — предыдущий, 1 — два назад.
    #[inline]
    pub fn past(&self, back: usize) -> Option<&Frame> {
        let idx = self.prev.len().checked_sub(back + 1)?;
        self.prev.get(idx).map(|f| f.as_ref())
    }

    /// Значение ползунка `i`, умноженное на chaos.
    #[inline]
    pub fn v(&self, i: usize) -> f32 {
        self.values.get(i) * self.chaos
    }

    /// Значение ползунка `i` без учёта chaos.
    #[inline]
    pub fn raw(&self, i: usize) -> f32 {
        self.values.get(i)
    }
}

/// Эффект — чистая функция `(кадр, параметры, seed) -> кадр`.
pub trait Effect: Send + Sync {
    /// Стабильный идентификатор, по нему сохраняются пресеты. Никогда не менять.
    fn id(&self) -> &'static str;

    /// Человекочитаемое имя для UI.
    fn name(&self) -> &'static str;

    fn group(&self) -> EffectGroup;

    /// Список ползунков. Длина и порядок — часть контракта пресетов.
    fn params(&self) -> &'static [crate::core::params::ParamSpec];

    /// Однострочное описание для подсказки.
    fn hint(&self) -> &'static str {
        ""
    }

    /// Применение. Ошибка возвращается наверх и показывается в UI — паник быть не должно.
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError>;

    /// Значения по умолчанию.
    fn defaults(&self) -> ParamValues {
        ParamValues::defaults_from(self.params())
    }
}
