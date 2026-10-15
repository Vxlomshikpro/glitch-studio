//! Тип кадра — плоский буфер RGBA8. Базовая единица пайплайна.

/// Плоский RGBA8-буфер: `data.len() == width * height * 4`.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// RGBA8, 4 байта на пиксель, построчно сверху вниз.
    pub data: Vec<u8>,
}

impl Frame {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    /// Создаёт кадр, проверяя длину буфера. `None` если длина не совпадает.
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if data.len() != expected {
            return None;
        }
        Some(Self {
            width,
            height,
            data,
        })
    }

    #[inline]
    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    #[inline]
    pub fn stride(&self) -> usize {
        self.width as usize * 4
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Индекс в буфере для координаты. `None` если координата вне кадра.
    #[inline]
    pub fn index(&self, x: i64, y: i64) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        Some((y as usize * self.width as usize + x as usize) * 4)
    }

    /// RGBA пикселя, `None` если за границами.
    #[inline]
    pub fn get(&self, x: i64, y: i64) -> Option<[u8; 4]> {
        let i = self.index(x, y)?;
        Some([
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ])
    }

    /// Запись RGBA. Выход за границы — тихий no-op, паник быть не должно ни при каких параметрах.
    #[inline]
    pub fn set(&mut self, x: i64, y: i64, px: [u8; 4]) {
        if let Some(i) = self.index(x, y) {
            self.data[i..i + 4].copy_from_slice(&px);
        }
    }

    /// Переносит содержимое `other` в self без аллокации, если размеры совпадают.
    pub fn copy_from(&mut self, other: &Frame) {
        if self.width == other.width && self.height == other.height {
            self.data.copy_from_slice(&other.data);
        }
    }

    /// Билинейная выборка с зажимом по краям. Возвращает RGB в 0..=255.
    #[inline]
    pub fn sample_bilinear(&self, x: f32, y: f32) -> [f32; 3] {
        let w = self.width as f32;
        let h = self.height as f32;
        let fx = (x.clamp(0.0, w - 1.0)).max(0.0);
        let fy = (y.clamp(0.0, h - 1.0)).max(0.0);
        let x0 = fx.floor();
        let y0 = fy.floor();
        let x1 = (x0 + 1.0).min(w - 1.0);
        let y1 = (y0 + 1.0).min(h - 1.0);
        let tx = fx - x0;
        let ty = fy - y0;

        let p00 = self.get_rgb(x0 as i64, y0 as i64);
        let p10 = self.get_rgb(x1 as i64, y0 as i64);
        let p01 = self.get_rgb(x0 as i64, y1 as i64);
        let p11 = self.get_rgb(x1 as i64, y1 as i64);

        let mut out = [0.0f32; 3];
        for c in 0..3 {
            let top = p00[c] + (p10[c] - p00[c]) * tx;
            let bot = p01[c] + (p11[c] - p01[c]) * tx;
            out[c] = top + (bot - top) * ty;
        }
        out
    }

    /// Линейное смешивание с `other` по `amount` (0..=1).
    /// Разные размеры — тихий no-op: в пайплайне такое невозможно,
    /// а эффектам нельзя паниковать на границе канвы.
    pub fn blend_from(&mut self, other: &Frame, amount: f32) {
        if self.width != other.width || self.height != other.height {
            return;
        }
        let a = amount.clamp(0.0, 1.0);
        let inv = 1.0 - a;
        for (dst, src) in self
            .data
            .chunks_exact_mut(4)
            .zip(other.data.chunks_exact(4))
        {
            for c in 0..4 {
                let d = dst[c] as f32;
                let s = src[c] as f32;
                dst[c] = (d * inv + s * a).round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Грязный доступ для эффектов, которые идут построчно.
    #[inline]
    pub fn row(&self, y: u32) -> &[u8] {
        let start = y as usize * self.stride();
        &self.data[start..start + self.stride()]
    }

    #[inline]
    pub fn row_mut(&mut self, y: u32) -> &mut [u8] {
        let start = y as usize * self.stride();
        let end = start + self.stride();
        &mut self.data[start..end]
    }

    #[inline]
    fn get_rgb(&self, x: i64, y: i64) -> [f32; 3] {
        match self.get(x, y) {
            Some(p) => [p[0] as f32, p[1] as f32, p[2] as f32],
            None => [0.0, 0.0, 0.0],
        }
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("len", &self.data.len())
            .finish()
    }
}
