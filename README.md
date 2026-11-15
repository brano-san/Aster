# Aster

<p align="center">
  <a href="https://github.com/brano-san/Aster/actions"><img src="https://img.shields.io/github/actions/workflow/status/brano-san/Aster/ci.yml?branch=master&style=flat-square&logo=github" alt="CI Status"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-stable-orange?style=flat-square&logo=rust" alt="Rust stable"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-blue?style=flat-square" alt="Platform">
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-green?style=flat-square" alt="License: MIT"></a>
</p>

## Fast, native GPU-accelerated terminal

**Aster — терминал для Windows и Linux, написанный на Rust.** Никакого Electron или WebView: интерфейс и сетка символов отрисовываются **собственным нативным GPU-пайплайном** (Direct3D 11 + DirectComposition + DirectWrite на Windows, OpenGL ES 3.0 + EGL + HarfRust + Swash на Linux под Wayland и X11) — тем же, что у файлового менеджера [Flit](https://github.com/brano-san/Flit).

Цель — возможности [Tabby](https://github.com/Eugeny/tabby) без его веса: вкладки и разделение на панели, профили и автоопределение оболочек (PowerShell, CMD, WSL, Git Bash, MSYS2, Cygwin), SSH и SFTP, последовательный порт и Telnet, хранилище секретов, режим Quake, цветовые схемы и гибкие горячие клавиши. Полный список — в [docs/FEATURES.md](docs/FEATURES.md).

## Status

Проект в ранней разработке. Текущий этап и порядок работ — в [docs/PLAN.md](docs/PLAN.md).

## Free, open source and private

Aster бесплатен и распространяется с открытым исходным кодом под лицензией [MIT](LICENSE). Никакой телеметрии, трекинга и рекламы: конфигурация, профили и секреты хранятся локально.

## Compiling from source

Нужен актуальный **Rust stable**.

#### Windows (MSVC)
```powershell
git clone https://github.com/brano-san/Aster.git
cd Aster
cargo build --release
# target\release\aster.exe
```

#### Linux (Wayland / X11)
Для запуска нужны EGL / OpenGL ES и XKB (на Ubuntu: `libegl1`, `libgles2`, `libgl1-mesa-dri`, `libxkbcommon-x11-0`), для сборки — компилятор C и стандартные инструменты.

```bash
cargo build --release -p aster
```

## Contributing

* **Issue Tracker:** [GitHub Issues](https://github.com/brano-san/Aster/issues) — ошибки и предложения.
* **Архитектура:** [ARCHITECTURE.md](ARCHITECTURE.md) — крейты, потоки и путь «ввод → кадр».
* **Рендеринг:** [docs/GPU-SHARED-LIB.md](docs/GPU-SHARED-LIB.md) — что взято из Flit и как это может стать общей библиотекой.

## License

Aster лицензирован под [MIT License](LICENSE).
