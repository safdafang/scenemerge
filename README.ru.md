# scenemerge

**Семантический diff и трёхстороннее слияние сцен и ресурсов Godot 4 (`.tscn`, `.tres`).**
Один бинарник, который подключается к Git как драйвер diff и merge, а также
работает в терминале и в CI.

> 🇬🇧 Полная документация — в [README.md](README.md) (на английском).

## Зачем

`.tscn` — текстовый формат, но Git видит в нём только строки. Редактор Godot
при сохранении перенумеровывает id ресурсов, переставляет секции, добавляет
идентификаторы — поэтому правка одной строки выглядит как стена изменений, а
два человека, поменявших *разные* узлы одной сцены, получают конфликт.

Godot не открывает файл с маркерами `<<<<<<<`, поэтому конфликты обычно
«решают», беря одну версию целиком и теряя чужую работу. **scenemerge**
понимает структуру сцены — узлы, свойства, ресурсы, сигналы — и сливает именно
её: изменения в разных узлах и свойствах объединяются автоматически,
перенумерация id не видна, а настоящий конфликт сводится к одной строке
свойства.

## Установка и настройка

Готовые бинарники для Linux, macOS и Windows — в
[Releases](https://github.com/OWNER/scenemerge/releases). Или через Cargo
(Rust 1.88+):

```sh
cargo install --git https://github.com/OWNER/scenemerge scenemerge
```

В репозитории проекта Godot:

```sh
scenemerge install            # для текущего репозитория
scenemerge install --global   # или для всех репозиториев пользователя
```

Команда добавляет строки в `.gitattributes` и регистрирует драйверы в
`git config`; повторный запуск ничего не дублирует, `scenemerge uninstall`
удаляет всё добавленное. Закоммитьте `.gitattributes`, а каждый участник
команды один раз выполняет `scenemerge install`.

## Пример

```console
$ scenemerge diff old.tscn new.tscn
scenes/level.tscn
~ Player/Camera3D
    transform.origin.y: 1 → 2.5
    fov: 75.0 → 90.0
✎ Enemies/Enemy renamed to Grunt
+ Enemies/Boss (instance of res://enemies/boss.tscn)
→ Pickups/Coin moved to Coins/Coin
- Enemies/OldSpawner (Node3D)
+ signal Player/Area3D.body_entered → Player._on_hit
```

При настоящем конфликте маркеры ставятся вокруг одного свойства:

```ini
[node name="Camera2D" type="Camera2D" parent="."]
<<<<<<< HEAD
zoom = Vector2(3, 3)
=======
zoom = Vector2(2, 2)
>>>>>>> feature
```

## Команды

| команда | назначение |
|---|---|
| `scenemerge diff OLD NEW` | семантический diff (`--format human\|json\|text`) |
| `scenemerge merge BASE OURS THEIRS -o OUT` | слияние; код 0 — чисто, 1 — конфликты, 2 — ошибка |
| `scenemerge textconv FILE` | каноническое представление для `git diff` |
| `scenemerge check ФАЙЛЫ/КАТАЛОГИ` | разбор, валидация и проверка lossless round-trip |
| `scenemerge install` / `uninstall` | настройка интеграции с Git |

Режимы конфликтов: `--conflict markers` (по умолчанию), `ours`, `theirs`;
JSON-отчёт — `--report conflicts.json` (схема: [docs/json-schema.md](docs/json-schema.md)).

## Ограничения

* Только Godot 4 (`format=3` и `format=4`); для файлов Godot 3 драйвер
  откатывается к обычному построчному слиянию Git.
* Значения `NodePath(...)` в свойствах не переписываются при переименовании
  узла в другой ветке.
* Массивы и словари сравниваются целиком (умный diff анимаций — в планах).

Решения и проверенные детали формата — в [DECISIONS.md](DECISIONS.md).
Лицензия — [MIT](LICENSE).
