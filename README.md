# Glitch Studio

Десктопный редактор для намеренной порчи фото и видео в стиле glitchcore.
Rust + egui/eframe, Linux (Wayland и X11), без root, без облаков.

Главная идея: один **seed** — один и тот же глитч. Никакой случайности «на каждый
кадр по-разному»: что видишь на превью, то и попадёт в экспорт.

---

## Запуск

```bash
./run.sh
```

Скрипт сам доставит зависимости, соберёт проект при необходимости и откроет окно.
Флаги:

```bash
./run.sh --debug          # быстрая пересборка в debug
GLITCH_BUILD_JOBS=6 ./run.sh   # если памяти хватает — собирать быстрее
```

Первый `release`-сборка занимает несколько минут, дальше запускается сразу.

### Ручная сборка

```bash
cargo build --release      # бинарь: target/release/glitch-studio
cargo test                 # юнит-тесты ядра и контрактные тесты всех эффектов
cargo clippy -- -D warnings
```

### Системные зависимости

```bash
sudo apt install ffmpeg libgl1-mesa-dev libxkbcommon-dev libwayland-dev
```

ffmpeg ставится системно и используется как внешний процесс для видео. Версию видно
в `ffmpeg -version`; нужна любая от 6.x.

---

## Горячие клавиши

| Клавиши | Действие |
|---|---|
| `Ctrl+O` | открыть файл |
| `Ctrl+Shift+O` | добавить папку в библиотеку |
| `Ctrl+S` | быстрое сохранение |
| `Ctrl+Shift+S` | Save As… |
| `Ctrl+E` | экспорт видео |
| `Ctrl+Z` / `Ctrl+Shift+Z` | отменить / повторить изменение настроек |
| `R` / `Shift+R` | новый seed |
| `Space` | play / pause видео |
| `←` `→` | шаг по кадрам |
| `B` (удерживать) | показать оригинал |
| `Ctrl+1…9` | загрузить пресет |
| `Ctrl+T` | следующая тема |
| `F11` | полноэкранный режим |
| `Tab` | скрыть панели |
| `Ctrl+/` | список горячих клавиш |

---

## Режим командной строки

```bash
glitch-studio --input photo.jpg --preset "VHS Death" --seed 42 --output out.png
glitch-studio --input clip.mp4 --preset "Datamosh Hell" --seed 7 --output out.mp4
glitch-studio --input photo.png --effect rgb_split --mix 70 --seed 1 --output out.png
glitch-studio --list-effects
glitch-studio --list-presets
```

Одинаковый `--seed` даёт побайтово одинаковый результат.

---

## Как добавить свой эффект

Эффект — чистая функция. Три шага:

**1. Создай файл в `src/effects/` или допиши в существующий:**

```rust
use crate::core::effect::{Effect, EffectCtx, EffectGroup};
use crate::core::error::GlitchError;
use crate::core::params::ParamSpec;

pub struct Invert;

impl Effect for Invert {
    fn id(&self) -> &'static str { "invert" }        // не менять: пресеты ссылаются на id
    fn name(&self) -> &'static str { "Invert" }
    fn group(&self) -> EffectGroup { EffectGroup::Color }
    fn params(&self) -> &'static [ParamSpec] {
        static P: [ParamSpec; 1] = [
            ParamSpec::new("amount", "Сила", 0.0, 100.0, 50.0, 1.0),
        ];
        &P
    }
    fn hint(&self) -> &'static str { "Инверсия цветов." }
    fn apply(&self, ctx: &mut EffectCtx<'_>) -> Result<(), GlitchError> {
        let amount = ctx.v(0) / 100.0;      // v(i) — с учётом Chaos
        for px in ctx.frame.as_mut_slice().chunks_exact_mut(4) {
            px[0] = (255.0 - px[0] as f32 * (1.0 - amount)) as u8;
            px[1] = (255.0 - px[1] as f32 * (1.0 - amount)) as u8;
            px[2] = (255.0 - px[2] as f32 * (1.0 - amount)) as u8;
        }
        Ok(())
    }
}
```

**2. Зарегистрируй** в `pub fn effects()` соответствующего файла-группы.

**3. Проверь:**

```bash
cargo test --test test_effects
```

Контрактные тесты сами проверят детерминизм, работу на 1x1, на крайних значениях
ползунков, при пустой истории кадров и на кадре 640x480. Если эффект паникует на
каком-то из этих случаев — сборка тестов упадёт.

### Правила, которые тесты проверяют автоматически

- Никаких `unwrap()`, `expect()`, выхода за границы и арифметики без `saturating_*`.
- Только случайность из `ctx.rng`. Никаких глобальных переменных и времени.
- Альфа-канал сохраняется.
- `id` — только snake_case и уникальный.
- Подпись параметра `hint()` непустая.

---

## Как добавить свою тему

Всё оформление идёт из одного модуля. Виджеты цветов вручную не задают.

Тема — это seed-цвет, из которого строится палитра Material You
(`primary`, `secondary`, `tertiary`, `surface` и контейнеры, `on-*`).

**1. Добавь тему в `src/theme/builtins.rs`:**

```rust
("Моя тема", MyTheme::seed()),
```

где `MyTheme::seed()` возвращает `[u8; 3]` — тот самый seed.

**2. Добавь seed-цвет в список тем** — он появится в переключателе тем и
автоматически получит светлый вариант.

Тёмный и светлый варианты строятся из одного seed: тёмный берет низкую
светлоту и высокую насыщенность, светлый — высокую светлоту и приглушённую.

**Динамические палитры** переключаются в настройках темы:

- **From Image** — палитра из текущего кадра; пересчитывается при смене файла.
- **From Wallpaper** — из обоев Hyprland (читается `hyprpaper.conf`).
- **Custom Color** — палитра из выбранного тобой seed-цвета.

---

## Где что лежит

```
src/core/      ядро: кадр, seed, параметры, пайплайн, пресеты, конфиг
src/effects/   эффекты по группам: color, geometry, pixels, artifacts, temporal
src/ui/        интерфейс: окно, превью, библиотека, инспектор, хоткеи
src/theme/     палитры Material You и glitchcore-оверлеи
src/video/     probe, декодер, энкодер через ffmpeg
src/library/   сканер папок и кеш миниатюр
tests/         тесты ядра и контрактные тесты эффектов
```

---

## Настройки и данные

Всё лежит в стандартных каталогах XDG (`~/.config/glitchstudio`,
`~/.cache/glitchstudio`). Там `settings.json` — тема, папки библиотеки,
история; `presets.json` — твои пресеты; кеш миниатюр можно безопасно удалить.

---

## Лицензия

MIT — свободное использование, модификация и распространение, в том числе
коммерческое. Единственное условие: указывать автора.

См. [LICENSE](LICENSE).
