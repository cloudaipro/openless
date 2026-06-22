# Build OpenLess on Ubuntu 24.04 (Noble)

Unofficial Linux build notes. Derived from the CI recipe
(`.github/workflows/release-tauri.yml`, which targets ubuntu-22.04) with the
deltas needed for 24.04. Produces `.deb` / `.rpm` / `.AppImage` plus the
fcitx5 input-method plugin that OpenLess uses for text insertion + global
hotkeys on Linux.

> Heads-up: Linux is a community/CI target, not a shipped Release. There is
> **no native local ASR engine on Linux** (Qwen is macOS-only, sherpa/Foundry
> are Windows-only). Use the local GPU Whisper server in
> `openless-all/scripts/local-whisper-server/` (Route A) for local speech-to-text.
>
> Traditional Chinese (台灣標準字) output, 中英混用 (晶晶體) normalization, and
> dictionary hotwords **all work on Linux** — they are engine-agnostic
> post-processing / prompt biasing (deps `ferrous-opencc` + `regex` are in the
> main `[dependencies]`, built on every platform). Only the local **sherpa**
> hotwords path is Windows-only. See
> `docs/hotwords-and-traditional-chinese.md` for the full design.

## 1. Toolchains

```bash
# Rust (stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
. "$HOME/.cargo/env"
rustc --version            # stable

# Node 20 (CI uses 20). nvm is simplest:
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
. ~/.nvm/nvm.sh
nvm install 20 && nvm use 20
node -v                    # v20.x
```

## 2. Tauri / bundle system deps

Same set as CI, with the **24.04 FUSE rename**: `libfuse2` → `libfuse2t64`
(the t64 time_t transition). AppImage needs FUSE2.

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  curl \
  file \
  libasound2-dev \
  libayatana-appindicator3-dev \
  libfuse2t64 \
  librsvg2-dev \
  libssl-dev \
  libwebkit2gtk-4.1-dev \
  libxdo-dev \
  patchelf \
  rpm \
  wget
```

`libwebkit2gtk-4.1-dev` (Tauri 2 requirement) is present in 24.04 — good.
If `libfuse2t64` can't be found, fall back to `sudo apt-get install -y libfuse2`.

## 3. Clone + submodules

```bash
git clone https://github.com/appergb/openless.git
cd openless
git submodule update --init --recursive   # pulls vendored qwen-asr (macOS-only, harmless on Linux)
```

## 4. Build the fcitx5 plugin

This is the Linux-specific piece. The plugin (`org.fcitx.Fcitx.OpenLess1`
DBus service) does `CommitText` into the focused input context and registers
the dictation hotkeys. Without it, insertion falls back to clipboard/enigo.

```bash
# fcitx5 dev packages (24.04 universe ships the split packages; the umbrella
# fcitx5-dev was dropped — install the components directly, same as CI):
sudo add-apt-repository -y universe
sudo apt-get update
sudo apt-get install -y \
  cmake \
  extra-cmake-modules \
  libfcitx5core-dev \
  libfcitx5utils-dev \
  libfcitx5config-dev \
  fcitx5-modules-dev \
  fcitx5 \
  fcitx5-module-dbus

cd openless-all/scripts/linux-fcitx5-plugin
# IMPORTANT: prefix /usr, not /usr/local — fcitx5 only searches
# /usr/lib/<arch>/fcitx5/ for addons.
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr
cmake --build build --parallel
sudo cmake --install build          # installs libopenless.so + openless.conf
cd ../../..
```

Restart fcitx5 to load it:

```bash
fcitx5 -rd     # restart (or log out / in)
```

(`build.sh install` does the configure+build+install in one shot if you prefer.)

## 5. Build the app

```bash
cd openless-all/app
npm ci

# Dev (Vite :1420 + Tauri shell):
npm run tauri dev

# Release bundles:
npm run tauri -- build --bundles deb,rpm,appimage
```

Artifacts land in `openless-all/app/src-tauri/target/release/bundle/`
(`deb/`, `rpm/`, `appimage/`).

### Bundling the plugin into the package (optional, matches CI)

CI copies the built plugin into the bundle so the `.deb`/`.rpm` install it to
the system fcitx5 dirs and the AppImage carries it as a resource (auto-installed
to `~/.local/` at runtime by `ensure_plugin_installed()`). To replicate, copy
the plugin artifacts and pass a bundle config override:

```bash
mkdir -p src-tauri/linux-fcitx5-plugin
cp ../scripts/linux-fcitx5-plugin/build/libopenless.so src-tauri/linux-fcitx5-plugin/
cp ../scripts/linux-fcitx5-plugin/build/openless.conf  src-tauri/linux-fcitx5-plugin/

# Discover the install dirs your CMake reported:
#   FCITX_INSTALL_ADDONDIR    e.g. /usr/lib/x86_64-linux-gnu/fcitx5
#   FCITX_INSTALL_PKGDATADIR  e.g. /usr/share/fcitx5
```

Then a `--config` JSON with `bundle.linux.deb.files` mapping
`<ADDONDIR>/libopenless.so` and `<PKGDATADIR>/addon/openless.conf`, plus
`bundle.resources: ["linux-fcitx5-plugin/libopenless.so"]` for AppImage — see
the exact shape in `release-tauri.yml` ("Build (Linux)" step). If you only build
for your own machine, step 4's `sudo cmake --install` already put the plugin in
place, so plain `--bundles deb,rpm,appimage` is enough.

## 6. Runtime setup

1. Use **fcitx5** as your input method (`im-config -n fcitx5`, then re-login).
2. Launch OpenLess. Grant microphone access.
3. Settings → Providers → ASR → **Whisper (OpenAI compatible)**:
   Base URL `http://127.0.0.1:8001/v1`, model `large-v3-turbo`, key `local`
   (with the Route A server running).
4. Settings → Providers → LLM: your OpenAI-compatible key for polish/translate.
5. Hold the hotkey and speak. If text doesn't insert, confirm fcitx5 is the
   active IM and the plugin loaded: `fcitx5-diagnose | grep -i openless`.

### Traditional Chinese (台灣) + hotwords on Linux

Works with the Route A Whisper server (or any cloud ASR) — these are
post-processing / prompt steps, independent of the recognition engine:

1. **Traditional output**: Settings → set the Chinese script preference to
   **繁體中文**. Output is converted with OpenCC `s2tw` (台灣標準字: 吃 not 喫,
   裡 not 裏) plus a Taiwan char-fix (賬→帳). Whisper emits mostly Simplified;
   the conversion is the deterministic guarantee — do **not** rely on Whisper to
   emit Traditional directly (its `zh` token has no Simplified/Traditional split).
   - One-shot path: full conversion + 中英混用 (晶晶體) normalization.
   - Streaming-insert path: per-flush char-level `s2tw` only (line-oriented
     English normalization needs whole-line context → one-shot only).
2. **Hotwords (proper-noun accuracy)**: Settings → 字典/Dictionary, add names,
   products, terms. On Linux these flow into the **Whisper `prompt`** as a
   recognition bias (sherpa `hotwords_file` is Windows-only). For deterministic
   output fixes, use **校正規則 / Correction Rules** instead (post-ASR replace).
   The input-bias vs output-fix distinction is documented in
   `docs/hotwords-and-traditional-chinese.md`.

## Troubleshooting

- **AppImage won't run** (`dlopen libfuse.so.2`): install `libfuse2t64`; or run
  with `./OpenLess_*.AppImage --appimage-extract-and-run`.
- **CMake can't find Fcitx5Core**: missing `libfcitx5core-dev` /
  `extra-cmake-modules` (fcitx5's CMake config needs KDE ECM).
- **Plugin builds but text won't insert**: prefix wasn't `/usr` → fcitx5 can't
  find the addon. Reconfigure with `-DCMAKE_INSTALL_PREFIX=/usr`, reinstall,
  `fcitx5 -rd`.
- **webkit/blank window**: ensure `libwebkit2gtk-4.1-dev` (not the old 4.0).
- **No local ASR option does anything**: expected — Linux has no in-process
  local engine; use the Route A GPU Whisper server.
```
