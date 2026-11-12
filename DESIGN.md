# RustyOS --- Design System

## 1. Направление

Визуальное вдохновение --- цельность, аккуратная типографика, ясная
иерархия и плавные переходы macOS. Не копировать брендинг, proprietary
assets или точные интерфейсы.

## 2. UX-принципы

- Предсказуемость важнее декоративности.
- Основные действия доступны быстро.
- Анимации короткие, осмысленные и interruptible.
- Поддерживать reduced-motion mode.
- Клавиатурная навигация и focus indication обязательны.
- Сохранять читаемость при разных scaling factors.
- Blur/transparency не должны ухудшать контраст или производительность.

## 3. Design tokens

Цвета, spacing, radius, typography и motion должны иметь единый источник
истины.

Начальный пример, не окончательный контракт:

``` toml
[colors]
background = "#0F0A19"
surface = "#161023"
accent = "#A56EFF"
text = "#F4F0FA"
muted = "#A69BB5"
danger = "#E56B75"

[radius]
small = 6
medium = 10
large = 16
pill = 999

[spacing]
unit = 4
small = 8
medium = 12
large = 20
xlarge = 28

[motion]
reduced_motion = false
short_ms = 120
standard_ms = 180
long_ms = 260
```

## 4. Motion and accessibility

Использовать monotonic time. Анимация должна корректно отменяться и не
задерживать критические действия. Проверять контраст, focus, keyboard
navigation, scaling, reduced motion и screen-reader integration.
