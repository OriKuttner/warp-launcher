# Warp Launcher 🚀

**Warp Launcher** is a lightweight, open-source desktop launcher and isolated runtime wrapper for Progressive Web Apps (PWAs), built with **Tauri v2** (Rust) and **Angular**.

It turns any web application or PWA into a true desktop experience with **completely isolated local storage and sessions**, auto-fetched icons, native desktop integration, and familiar browser keyboard shortcuts.

---

## ✨ Features

- **🛡️ 100% Isolated Profiles & Storage**:
  - Each app profile gets its own dedicated data directory (`~/.local/share/warp/profiles/<folder-id>`).
  - Isolate `localStorage`, `sessionStorage`, `Cookies`, and `IndexedDB`.
  - Run multiple different accounts for the exact same service (e.g., personal & work accounts) side-by-side with zero data leakage.

- **🚀 Central Launcher UI**:
  - Clean, responsive dashboard built with Angular.
  - Automatically adapts to system **Dark / Light theme** (`color-scheme`).
  - Add, manage, and launch your PWAs in one click.

- **🎨 Automatic Favicon Fetching**:
  - Warp scrapes and caches the site's manifest and favicon locally upon adding the app.

- **⌨️ Browser Keyboard Shortcuts Injected**:
  - `F5` / `Ctrl + R`: Reload page.
  - `Ctrl + +` / `Ctrl + -`: Zoom in / Zoom out (saved per-profile).
  - `Ctrl + 0`: Reset zoom.
  - `Alt + ←` / `Alt + →`: Browser history navigation (Back / Forward).

- **🐧 Linux Desktop Integration**:
  - Create native `.desktop` shortcuts in `~/.local/share/applications/` directly from the UI.
  - Dedicated `StartupWMClass` ensures separate grouping in the system dock / taskbar.
  - CLI support: launch specific apps directly with `warp --app <folder-id>`.

---

## 🛠️ Tech Stack

- **Frontend**: [Angular 21](https://angular.dev/) (Standalone Components, Signals)
- **Desktop Runtime**: [Tauri v2](https://v2.tauri.app/) (Rust, WebKitGTK on Linux)
- **Data Store**: Native JSON store in `~/.local/share/warp/apps.json`

---

## 📦 Prerequisites

Make sure you have installed:
- **Node.js** (v18+) and **npm**
- **Rust & Cargo** (1.78+)
- **Linux WebKitGTK & dependencies**:
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

---

## 🚀 Getting Started

### 1. Clone the repository
```bash
git clone git@github.com:OriKuttner/warp-launcher.git
cd warp-launcher
```

### 2. Install dependencies
```bash
npm install
```

### 3. Run in Development Mode
```bash
npm run tauri dev
```

### 4. Build Production Binary & Packages
To build an optimized binary and native Linux packages (`.deb`, `.rpm`, `.AppImage`):
```bash
npm run tauri build
```

The resulting packages will be located in:
```
src-tauri/target/release/bundle/
```

To install the `.deb` package on Ubuntu/Debian:
```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/warp_0.1.0_amd64.deb
```

---

## 📄 License

This project is licensed under the **GNU General Public License v3.0 (GPLv3)** - see the [LICENSE](LICENSE) file for details.
