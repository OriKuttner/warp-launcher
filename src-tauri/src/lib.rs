use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{webview::NewWindowResponse, AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PwaApp {
    pub id: String,
    pub name: String,
    pub url: String,
    pub icon: Option<String>,
    #[serde(default)]
    pub notifications: Option<String>,
}

fn get_warp_dir() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or_else(|| "Could not find user data directory".to_string())?;
    let warp_dir = data_dir.join("warp");
    if !warp_dir.exists() {
        fs::create_dir_all(&warp_dir).map_err(|e| e.to_string())?;
    }
    Ok(warp_dir)
}

fn get_apps_file_path() -> Result<PathBuf, String> {
    let warp_dir = get_warp_dir()?;
    Ok(warp_dir.join("apps.json"))
}

fn read_apps() -> Result<Vec<PwaApp>, String> {
    let path = get_apps_file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let apps: Vec<PwaApp> = serde_json::from_str(&content).unwrap_or_default();
    Ok(apps)
}

fn write_apps(apps: &[PwaApp]) -> Result<(), String> {
    let path = get_apps_file_path()?;
    let content = serde_json::to_string_pretty(apps).map_err(|e| e.to_string())?;
    fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn sanitize_id(raw: &str) -> String {
    let mut clean: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if clean.is_empty() {
        clean = "app".to_string();
    }
    clean
}

fn fetch_favicon(app_url: &str, app_id: &str) -> Option<String> {
    let warp_dir = get_warp_dir().ok()?;
    let icons_dir = warp_dir.join("icons");
    if !icons_dir.exists() {
        let _ = fs::create_dir_all(&icons_dir);
    }

    let parsed_url = reqwest::Url::parse(app_url).ok()?;
    let icon_filename = format!("{}.png", app_id);
    let target_icon_path = icons_dir.join(&icon_filename);

    // Try finding favicon from html or standard favicon.ico
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .ok()?;

    let mut icon_url: Option<reqwest::Url> = None;

    if let Ok(resp) = client.get(app_url).send() {
        if let Ok(html) = resp.text() {
            let document = scraper::Html::parse_document(&html);
            if let Ok(selector) = scraper::Selector::parse("link[rel*='icon'], link[rel='apple-touch-icon']") {
                for element in document.select(&selector) {
                    if let Some(href) = element.value().attr("href") {
                        if let Ok(resolved) = parsed_url.join(href) {
                            icon_url = Some(resolved);
                            break;
                        }
                    }
                }
            }
        }
    }

    let candidate_url = icon_url.unwrap_or_else(|| {
        parsed_url.join("/favicon.ico").unwrap_or_else(|_| parsed_url.clone())
    });

    if let Ok(resp) = client.get(candidate_url).send() {
        if resp.status().is_success() {
            if let Ok(bytes) = resp.bytes() {
                if !bytes.is_empty() {
                    let _ = fs::write(&target_icon_path, bytes);
                    return Some(target_icon_path.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

fn icon_to_data_url(path_str: &str) -> Option<String> {
    let path = std::path::Path::new(path_str);
    if path.exists() {
        if let Ok(bytes) = fs::read(path) {
            let mime = if path_str.ends_with(".ico") {
                "image/x-icon"
            } else if path_str.ends_with(".svg") {
                "image/svg+xml"
            } else {
                "image/png"
            };
            let b64 = use_base64(&bytes);
            return Some(format!("data:{};base64,{}", mime, b64));
        }
    }
    None
}

fn use_base64(bytes: &[u8]) -> String {
    // Simple base64 encode using std without extra dependencies
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        result.push(CHARSET[((n >> 18) & 63) as usize] as char);
        result.push(CHARSET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARSET[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARSET[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[tauri::command]
fn get_apps() -> Result<Vec<PwaApp>, String> {
    let mut apps = read_apps()?;
    for app in &mut apps {
        if let Some(ref icon_path) = app.icon {
            if !icon_path.starts_with("data:") {
                app.icon = icon_to_data_url(icon_path).or_else(|| app.icon.clone());
            }
        }
    }
    Ok(apps)
}

fn write_desktop_shortcut(app: &PwaApp) -> Result<String, String> {
    let data_dir = dirs::data_dir().ok_or_else(|| "Could not find data directory".to_string())?;
    let applications_dir = data_dir.join("applications");
    if !applications_dir.exists() {
        fs::create_dir_all(&applications_dir).map_err(|e| e.to_string())?;
    }

    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let desktop_file_path = applications_dir.join(format!("warp-{}.desktop", app.id));

    let icon_line = if let Some(icon) = &app.icon {
        format!("Icon={}\n", icon)
    } else {
        "Icon=warp\n".to_string()
    };

    let desktop_content = format!(
        "[Desktop Entry]\n\
        Type=Application\n\
        Name={name}\n\
        Comment=PWA running with Warp\n\
        Exec={exe} --app {id}\n\
        {icon_line}\
        Terminal=false\n\
        Categories=Network;WebBrowser;\n\
        StartupWMClass=warp-{id}\n",
        name = app.name,
        exe = current_exe.to_string_lossy(),
        id = app.id,
        icon_line = icon_line
    );

    fs::write(&desktop_file_path, desktop_content).map_err(|e| e.to_string())?;
    Ok(desktop_file_path.to_string_lossy().to_string())
}

#[tauri::command]
fn save_app(
    name: String,
    url: String,
    folder_id: String,
    is_edit: Option<bool>,
    old_folder_id: Option<String>,
) -> Result<PwaApp, String> {
    let id = sanitize_id(&folder_id);
    let mut apps = read_apps()?;
    let is_editing = is_edit.unwrap_or(false);

    let warp_dir = get_warp_dir()?;
    let profiles_dir = warp_dir.join("profiles");
    let icons_dir = warp_dir.join("icons");

    let existing_idx = if is_editing {
        let lookup_id = old_folder_id
            .as_deref()
            .map(sanitize_id)
            .unwrap_or_else(|| id.clone());
        apps.iter().position(|a| a.id == lookup_id)
    } else {
        apps.iter().position(|a| a.id == id)
    };

    if !is_editing && existing_idx.is_some() {
        return Err(format!(
            "A profile directory with the name '{}' already exists. Please choose a different directory name.",
            id
        ));
    }

    if is_editing && existing_idx.is_none() {
        return Err("Application profile was not found to update.".to_string());
    }

    // If editing and folder_id has changed, migrate profile folder, icon and shortcut
    if is_editing {
        if let Some(idx) = existing_idx {
            let old_id = apps[idx].id.clone();
            if old_id != id {
                // Ensure new id is not already used by another app
                if apps.iter().any(|a| a.id == id) {
                    return Err(format!(
                        "A profile directory with the name '{}' already exists. Please choose a different directory name.",
                        id
                    ));
                }

                // Migrate profile folder on disk
                let old_profile_dir = profiles_dir.join(&old_id);
                let new_profile_dir = profiles_dir.join(&id);

                if old_profile_dir.exists() {
                    if new_profile_dir.exists() {
                        return Err(format!(
                            "Directory '{}' already exists on disk. Please choose another directory name.",
                            id
                        ));
                    }
                    fs::rename(&old_profile_dir, &new_profile_dir).map_err(|e| {
                        format!("Failed to move profile data from '{}' to '{}': {}", old_id, id, e)
                    })?;
                }

                // Migrate icon file if present
                let old_icon_path = icons_dir.join(format!("{}.png", old_id));
                let new_icon_path = icons_dir.join(format!("{}.png", id));
                if old_icon_path.exists() && !new_icon_path.exists() {
                    let _ = fs::rename(&old_icon_path, &new_icon_path);
                }

                // If desktop shortcut exists, migrate it
                if let Some(data_dir) = dirs::data_dir() {
                    let applications_dir = data_dir.join("applications");
                    let old_desktop = applications_dir.join(format!("warp-{}.desktop", old_id));
                    if old_desktop.exists() {
                        let _ = fs::remove_file(old_desktop);
                        let updated_app = PwaApp {
                            id: id.clone(),
                            name: name.trim().to_string(),
                            url: url.trim().to_string(),
                            icon: if new_icon_path.exists() {
                                Some(new_icon_path.to_string_lossy().to_string())
                            } else {
                                apps[idx].icon.clone()
                            },
                            notifications: apps[idx].notifications.clone(),
                        };
                        let _ = write_desktop_shortcut(&updated_app);
                    }
                }
            }
        }
    }

    // Check if updating or adding
    let icon_path = fetch_favicon(&url, &id);
    let existing_notifications = existing_idx.and_then(|idx| apps[idx].notifications.clone());

    let mut app = PwaApp {
        id: id.clone(),
        name: name.trim().to_string(),
        url: url.trim().to_string(),
        icon: icon_path,
        notifications: existing_notifications,
    };

    if let Some(idx) = existing_idx {
        apps[idx].id = app.id.clone();
        apps[idx].name = app.name.clone();
        apps[idx].url = app.url.clone();
        if app.icon.is_some() {
            apps[idx].icon = app.icon.clone();
        } else {
            // Keep existing icon if new fetch didn't return one, updating path if id changed
            if let Some(ref _old_icon) = apps[idx].icon {
                let new_icon_str = format!("{}.png", id);
                let new_icon_path = icons_dir.join(&new_icon_str);
                if new_icon_path.exists() {
                    apps[idx].icon = Some(new_icon_path.to_string_lossy().to_string());
                }
            }
            app.icon = apps[idx].icon.clone();
        }
    } else {
        apps.push(app.clone());
    }

    write_apps(&apps)?;

    // Return data url to frontend
    if let Some(ref path) = app.icon {
        app.icon = icon_to_data_url(path).or_else(|| app.icon.clone());
    }
    Ok(app)
}

#[tauri::command]
fn set_app_notification_permission(id: String, permission: String) -> Result<(), String> {
    let mut apps = read_apps()?;
    if let Some(app) = apps.iter_mut().find(|a| a.id == id) {
        app.notifications = if permission == "default" {
            None
        } else {
            Some(permission)
        };
        write_apps(&apps)?;
        Ok(())
    } else {
        Err("App not found".to_string())
    }
}

#[tauri::command]
fn delete_app(id: String) -> Result<(), String> {
    let mut apps = read_apps()?;
    apps.retain(|a| a.id != id);
    write_apps(&apps)?;
    Ok(())
}

#[tauri::command]
fn reorder_apps(app_ids: Vec<String>) -> Result<(), String> {
    let current_apps = read_apps()?;
    let mut reordered: Vec<PwaApp> = Vec::new();

    for id in &app_ids {
        if let Some(app) = current_apps.iter().find(|a| &a.id == id) {
            reordered.push(app.clone());
        }
    }

    // Keep any apps that weren't in the provided list
    for app in current_apps {
        if !reordered.iter().any(|a| a.id == app.id) {
            reordered.push(app);
        }
    }

    write_apps(&reordered)?;
    Ok(())
}

#[tauri::command]
fn launch_app(app_handle: AppHandle, id: String) -> Result<(), String> {
    let apps = read_apps()?;
    let app = apps.into_iter().find(|a| a.id == id).ok_or_else(|| "App not found".to_string())?;

    open_pwa_window(&app_handle, &app)
}

#[tauri::command]
fn create_desktop_shortcut(id: String) -> Result<String, String> {
    let apps = read_apps()?;
    let app = apps.into_iter().find(|a| a.id == id).ok_or_else(|| "App not found".to_string())?;

    write_desktop_shortcut(&app)
}

fn open_in_default_browser(url: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
}

fn is_internal_url(target_url: &url::Url, base_host: Option<&str>) -> bool {
    let scheme = target_url.scheme();
    if scheme != "http" && scheme != "https" {
        return true;
    }
    if let Some(base) = base_host {
        if let Some(target_host) = target_url.host_str() {
            let target_host = target_host.to_lowercase();
            if target_host == base || target_host.ends_with(&format!(".{}", base)) {
                return true;
            }
            if let Some(dot_idx) = base.find('.') {
                let root_domain = &base[dot_idx + 1..];
                if root_domain.contains('.') && (target_host == root_domain || target_host.ends_with(&format!(".{}", root_domain))) {
                    return true;
                }
            }
        }
        return false;
    }
    true
}

pub fn open_pwa_window(app_handle: &AppHandle, app: &PwaApp) -> Result<(), String> {
    let window_label = format!("pwa-{}", app.id);

    // If window already exists, bring it to focus
    if let Some(existing_window) = app_handle.get_webview_window(&window_label) {
        let _ = existing_window.show();
        let _ = existing_window.set_focus();
        return Ok(());
    }

    let parsed_url: url::Url = app.url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let warp_dir = get_warp_dir()?;
    let profile_dir = warp_dir.join("profiles").join(&app.id);
    if !profile_dir.exists() {
        let _ = fs::create_dir_all(&profile_dir);
    }

    let shortcut_script = r#"
    (function() {
      // 0. Safe Storage with memory fallback (protects against SecurityError on about:blank or restricted origins)
      const memStore = {};
      const safeStorage = {
        getItem: function(k) {
          try {
            if (window.localStorage) {
              const val = window.localStorage.getItem(k);
              if (val !== null) return val;
            }
          } catch (e) {}
          return Object.prototype.hasOwnProperty.call(memStore, k) ? memStore[k] : null;
        },
        setItem: function(k, v) {
          try {
            if (window.localStorage) window.localStorage.setItem(k, v);
          } catch (e) {}
          memStore[k] = String(v);
        },
        removeItem: function(k) {
          try {
            if (window.localStorage) window.localStorage.removeItem(k);
          } catch (e) {}
          delete memStore[k];
        }
      };

      // 1. Mock PWA Standalone environment for all web applications
      try {
        Object.defineProperty(Navigator.prototype, 'standalone', {
          get: function() { return true; },
          configurable: true,
          enumerable: true
        });
      } catch (e) {}

      try {
        Object.defineProperty(navigator, 'standalone', {
          get: function() { return true; },
          configurable: true,
          enumerable: true
        });
      } catch (e) {}

      window.__WARP_LAUNCHER__ = true;
      safeStorage.setItem('isInstalled', 'true');

      try {
        const origMatchMedia = window.matchMedia;
        const mockMatchMedia = function(query) {
          const q = (typeof query === 'string') ? query.toLowerCase() : '';
          if (q.includes('display-mode: standalone') || 
              q.includes('display-mode:standalone') ||
              q.includes('display-mode: minimal-ui') ||
              q.includes('display-mode: fullscreen')) {
            return {
              matches: true,
              media: query,
              onchange: null,
              addListener: function() {},
              removeListener: function() {},
              addEventListener: function() {},
              removeEventListener: function() {},
              dispatchEvent: function() { return false; }
            };
          }
          return origMatchMedia ? origMatchMedia.call(window, query) : { matches: false, media: query };
        };

        try {
          Object.defineProperty(Window.prototype, 'matchMedia', {
            value: mockMatchMedia,
            writable: true,
            configurable: true
          });
        } catch (e) {}

        window.matchMedia = mockMatchMedia;
      } catch (e) {}

      // 1.5. Audio & Media Autoplay Enhancements
      try {
        const unlockAudio = function() {
          try {
            const AudioContextClass = window.AudioContext || window.webkitAudioContext;
            if (AudioContextClass) {
              if (!window.__warp_audio_ctx) {
                window.__warp_audio_ctx = new AudioContextClass();
              }
              if (window.__warp_audio_ctx.state === 'suspended') {
                window.__warp_audio_ctx.resume();
              }
            }
          } catch (e) {}
        };
        window.addEventListener('click', unlockAudio, { capture: true, passive: true });
        window.addEventListener('keydown', unlockAudio, { capture: true, passive: true });
        window.addEventListener('pointerdown', unlockAudio, { capture: true, passive: true });

        const origMediaPlay = HTMLMediaElement.prototype.play;
        HTMLMediaElement.prototype.play = function() {
          try {
            if (this.ended) {
              this.currentTime = 0;
            }
            if (this.readyState === 0 && this.preload === 'none') {
              this.load();
            }
          } catch (e) {}
          return origMediaPlay.apply(this, arguments);
        };
      } catch (e) {}

      // 2. Restore saved zoom level
      let currentZoom = parseFloat(safeStorage.getItem('__warp_zoom') || '1.0');
      if (currentZoom !== 1.0) {
        try {
          document.documentElement.style.zoom = currentZoom;
        } catch (e) {}
      }

      // 3. Window Title Customization (Persistent)
      let customTitle = safeStorage.getItem('__warp_custom_title') || '';
      let currentTitleVal = document.title;

      try {
        const titleDescriptor = Object.getOwnPropertyDescriptor(Document.prototype, 'title') ||
                                Object.getOwnPropertyDescriptor(HTMLDocument.prototype, 'title');

        if (titleDescriptor && titleDescriptor.set) {
          const origTitleSet = titleDescriptor.set;
          const origTitleGet = titleDescriptor.get;

          Object.defineProperty(document, 'title', {
            get: function() {
              return customTitle || (origTitleGet ? origTitleGet.call(this) : currentTitleVal);
            },
            set: function(val) {
              currentTitleVal = val;
              if (customTitle) {
                if (origTitleSet) origTitleSet.call(this, customTitle);
              } else {
                if (origTitleSet) origTitleSet.call(this, val);
              }
            },
            configurable: true
          });
        }
      } catch (e) {}

      if (customTitle) {
        try { document.title = customTitle; } catch (e) {}
      }

      function showTitleDialog() {
        if (document.getElementById('__warp_title_dialog')) {
          const existingInp = document.getElementById('__warp_title_input');
          if (existingInp) { existingInp.focus(); existingInp.select(); }
          return;
        }

        const overlay = document.createElement('div');
        overlay.id = '__warp_title_dialog';
        overlay.style.cssText = 'position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.65);z-index:2147483647;display:flex;align-items:center;justify-content:center;font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;';

        const box = document.createElement('div');
        box.style.cssText = 'background:#1e293b;color:#f8fafc;padding:24px;border-radius:12px;box-shadow:0 20px 25px -5px rgba(0,0,0,0.5);width:90%;max-width:500px;border:1px solid #334155;';

        const label = document.createElement('label');
        label.htmlFor = '__warp_title_input';
        label.innerText = 'Change Window Title:';
        label.style.cssText = 'display:block;margin-bottom:8px;font-size:14px;font-weight:600;color:#e2e8f0;';

        const input = document.createElement('input');
        input.id = '__warp_title_input';
        input.type = 'text';
        input.value = customTitle || document.title || '';
        input.style.cssText = 'width:100%;box-sizing:border-box;padding:10px 14px;border:1px solid #475569;border-radius:8px;background:#0f172a;color:#f8fafc;font-size:14px;outline:none;';

        const hint = document.createElement('div');
        hint.innerText = 'Leave empty to restore default website title';
        hint.style.cssText = 'font-size:12px;color:#94a3b8;margin-top:6px;';

        const btnRow = document.createElement('div');
        btnRow.style.cssText = 'display:flex;justify-content:flex-end;gap:10px;margin-top:16px;';

        const cancelBtn = document.createElement('button');
        cancelBtn.type = 'button';
        cancelBtn.innerText = 'Cancel';
        cancelBtn.style.cssText = 'padding:8px 16px;border-radius:6px;border:1px solid #475569;background:transparent;color:#94a3b8;cursor:pointer;font-size:13px;font-weight:500;';

        const saveBtn = document.createElement('button');
        saveBtn.type = 'button';
        saveBtn.innerText = 'Save';
        saveBtn.style.cssText = 'padding:8px 20px;border-radius:6px;border:none;background:#3b82f6;color:#ffffff;cursor:pointer;font-size:13px;font-weight:600;';

        function close() {
          if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
        }

        function save() {
          const val = input.value.trim();
          if (val) {
            customTitle = val;
            safeStorage.setItem('__warp_custom_title', val);
            document.title = val;
          } else {
            customTitle = '';
            safeStorage.removeItem('__warp_custom_title');
            document.title = currentTitleVal || '';
          }
          close();
        }

        cancelBtn.onclick = close;
        saveBtn.onclick = save;

        input.onkeydown = function(ev) {
          if (ev.key === 'Enter') {
            ev.preventDefault();
            save();
          } else if (ev.key === 'Escape') {
            ev.preventDefault();
            close();
          }
        };

        overlay.onclick = function(ev) {
          if (ev.target === overlay) close();
        };

        btnRow.appendChild(cancelBtn);
        btnRow.appendChild(saveBtn);
        box.appendChild(label);
        box.appendChild(input);
        box.appendChild(hint);
        box.appendChild(btnRow);
        overlay.appendChild(box);
        (document.body || document.documentElement).appendChild(overlay);

        input.focus();
        input.select();
      }

      // 4. Floating URL Navigation Dialog (Ctrl + L or F6)
      function showUrlDialog() {
        if (document.getElementById('__warp_url_dialog')) {
          const existingInp = document.getElementById('__warp_url_input');
          if (existingInp) { existingInp.focus(); existingInp.select(); }
          return;
        }

        const overlay = document.createElement('div');
        overlay.id = '__warp_url_dialog';
        overlay.style.cssText = 'position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.65);z-index:2147483647;display:flex;align-items:center;justify-content:center;font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;';

        const box = document.createElement('div');
        box.style.cssText = 'background:#1e293b;color:#f8fafc;padding:24px;border-radius:12px;box-shadow:0 20px 25px -5px rgba(0,0,0,0.5);width:90%;max-width:540px;border:1px solid #334155;';

        const label = document.createElement('label');
        label.htmlFor = '__warp_url_input';
        label.innerText = 'Navigate to URL or Login Link:';
        label.style.cssText = 'display:block;margin-bottom:8px;font-size:14px;font-weight:600;color:#e2e8f0;';

        const input = document.createElement('input');
        input.id = '__warp_url_input';
        input.type = 'text';
        input.value = window.location.href;
        input.style.cssText = 'width:100%;box-sizing:border-box;padding:10px 14px;border:1px solid #475569;border-radius:8px;background:#0f172a;color:#f8fafc;font-size:14px;outline:none;';

        const btnRow = document.createElement('div');
        btnRow.style.cssText = 'display:flex;justify-content:flex-end;gap:10px;margin-top:16px;';

        const cancelBtn = document.createElement('button');
        cancelBtn.type = 'button';
        cancelBtn.innerText = 'Cancel';
        cancelBtn.style.cssText = 'padding:8px 16px;border-radius:6px;border:1px solid #475569;background:transparent;color:#94a3b8;cursor:pointer;font-size:13px;font-weight:500;';

        const goBtn = document.createElement('button');
        goBtn.type = 'button';
        goBtn.innerText = 'Go';
        goBtn.style.cssText = 'padding:8px 20px;border-radius:6px;border:none;background:#3b82f6;color:#ffffff;cursor:pointer;font-size:13px;font-weight:600;';

        function close() {
          if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
        }

        function navigate() {
          let val = input.value.trim();
          if (val) {
            if (!val.startsWith('http://') && !val.startsWith('https://')) {
              val = 'https://' + val;
            }
            close();
            window.location.href = val;
          }
        }

        cancelBtn.onclick = close;
        goBtn.onclick = navigate;

        input.onkeydown = function(ev) {
          if (ev.key === 'Enter') {
            ev.preventDefault();
            navigate();
          } else if (ev.key === 'Escape') {
            ev.preventDefault();
            close();
          }
        };

        overlay.onclick = function(ev) {
          if (ev.target === overlay) close();
        };

        const permBtn = document.createElement('button');
        permBtn.type = 'button';
        permBtn.innerText = '🔔 Permissions (F8)';
        permBtn.style.cssText = 'padding:8px 14px;border-radius:6px;border:1px solid #475569;background:transparent;color:#93c5fd;cursor:pointer;font-size:13px;font-weight:500;margin-right:auto;';
        permBtn.onclick = function() {
          close();
          showPermissionsDialog();
        };

        btnRow.appendChild(permBtn);
        btnRow.appendChild(cancelBtn);
        btnRow.appendChild(goBtn);
        box.appendChild(label);
        box.appendChild(input);
        box.appendChild(btnRow);
        overlay.appendChild(box);
        (document.body || document.documentElement).appendChild(overlay);

        input.focus();
        input.select();
      }

      // 5. Desktop Notifications & Site Permissions
      const activePermissionStatuses = [];

      function getWarpPermission() {
        const saved = safeStorage.getItem('__warp_notification_permission');
        if (saved === 'granted' || saved === 'denied' || saved === 'default') {
          return saved;
        }
        return window.__WARP_INITIAL_NOTIF_PERM__ || 'default';
      }

      function notifyPermissionChanged(newPerm) {
        const newState = (newPerm === 'granted') ? 'granted' : ((newPerm === 'denied') ? 'denied' : 'prompt');
        for (let i = 0; i < activePermissionStatuses.length; i++) {
          const s = activePermissionStatuses[i];
          s.state = newState;
          s.status = newState;
          if (typeof s.onchange === 'function') {
            try { s.onchange(new Event('change')); } catch (e) {}
          }
          try { s.dispatchEvent(new Event('change')); } catch (e) {}
        }
      }

      // Intercept Permissions API: navigator.permissions.query
      try {
        if (!navigator.permissions) {
          try {
            Object.defineProperty(navigator, 'permissions', {
              value: {},
              writable: true,
              configurable: true
            });
          } catch (e) {}
        }
        if (navigator.permissions) {
          const origQuery = (typeof navigator.permissions.query === 'function') ? navigator.permissions.query.bind(navigator.permissions) : null;
          navigator.permissions.query = function(desc) {
            if (desc && (desc.name === 'notifications' || desc.name === 'push')) {
              const perm = getWarpPermission();
              const state = (perm === 'granted') ? 'granted' : ((perm === 'denied') ? 'denied' : 'prompt');
              const target = new EventTarget();
              const statusObj = Object.assign(target, {
                state: state,
                status: state,
                name: desc.name,
                onchange: null
              });
              activePermissionStatuses.push(statusObj);
              return Promise.resolve(statusObj);
            }
            if (origQuery) return origQuery(desc);
            return Promise.reject(new TypeError("Failed to execute 'query' on 'Permissions': The provided value '" + (desc && desc.name) + "' is not a valid enum value of type PermissionName."));
          };
        }
      } catch (e) {}

      function showNotificationToast(title, options) {
        options = options || {};
        let container = document.getElementById('__warp_toast_container');
        if (!container) {
          container = document.createElement('div');
          container.id = '__warp_toast_container';
          container.style.cssText = 'position:fixed;bottom:24px;right:24px;z-index:2147483647;display:flex;flex-direction:column;gap:12px;max-width:380px;pointer-events:none;font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;';
          (document.body || document.documentElement).appendChild(container);
        }

        const toast = document.createElement('div');
        toast.style.cssText = 'pointer-events:auto;background:#1e293b;color:#f8fafc;padding:14px 16px;border-radius:10px;box-shadow:0 12px 28px rgba(0,0,0,0.45);border:1px solid #3b82f6;display:flex;gap:12px;align-items:flex-start;';

        const iconEl = document.createElement('div');
        if (options.icon) {
          const img = document.createElement('img');
          img.src = options.icon;
          img.style.cssText = 'width:32px;height:32px;border-radius:6px;object-fit:contain;';
          iconEl.appendChild(img);
        } else {
          iconEl.innerText = '🔔';
          iconEl.style.cssText = 'font-size:22px;line-height:1;';
        }

        const bodyEl = document.createElement('div');
        bodyEl.style.cssText = 'flex:1;min-width:0;';

        const titleDiv = document.createElement('div');
        titleDiv.innerText = title || 'Notification';
        titleDiv.style.cssText = 'font-weight:600;font-size:14px;color:#f8fafc;margin-bottom:4px;word-break:break-word;';

        const descDiv = document.createElement('div');
        descDiv.innerText = options.body || '';
        descDiv.style.cssText = 'font-size:13px;color:#cbd5e1;line-height:1.4;word-break:break-word;';

        bodyEl.appendChild(titleDiv);
        if (options.body) bodyEl.appendChild(descDiv);

        const closeBtn = document.createElement('button');
        closeBtn.type = 'button';
        closeBtn.innerText = '✕';
        closeBtn.style.cssText = 'background:transparent;border:none;color:#94a3b8;cursor:pointer;font-size:14px;padding:0;line-height:1;margin-left:4px;';

        function removeToast() {
          toast.style.opacity = '0';
          toast.style.transform = 'translateY(8px)';
          toast.style.transition = 'opacity 0.2s ease, transform 0.2s ease';
          setTimeout(() => {
            if (toast.parentNode) toast.parentNode.removeChild(toast);
          }, 200);
        }

        closeBtn.onclick = function(e) {
          e.stopPropagation();
          removeToast();
        };

        toast.onclick = function() {
          window.focus();
          removeToast();
        };

        toast.appendChild(iconEl);
        toast.appendChild(bodyEl);
        toast.appendChild(closeBtn);
        container.appendChild(toast);

        setTimeout(removeToast, 6000);
      }

      function showPermissionsDialog() {
        if (document.getElementById('__warp_perms_dialog')) {
          return;
        }

        const overlay = document.createElement('div');
        overlay.id = '__warp_perms_dialog';
        overlay.style.cssText = 'position:fixed;top:0;left:0;width:100vw;height:100vh;background:rgba(0,0,0,0.65);z-index:2147483647;display:flex;align-items:center;justify-content:center;font-family:-apple-system,BlinkMacSystemFont,Segoe UI,Roboto,sans-serif;';

        const box = document.createElement('div');
        box.style.cssText = 'background:#1e293b;color:#f8fafc;padding:24px;border-radius:12px;box-shadow:0 20px 25px -5px rgba(0,0,0,0.5);width:90%;max-width:520px;border:1px solid #334155;';

        const title = document.createElement('h3');
        title.innerText = 'Site Permissions: ' + (window.location.hostname || 'Current Site');
        title.style.cssText = 'margin:0 0 16px 0;font-size:16px;font-weight:600;color:#f8fafc;display:flex;align-items:center;gap:8px;';

        const row = document.createElement('div');
        row.style.cssText = 'background:#0f172a;border:1px solid #334155;border-radius:8px;padding:14px 16px;margin-bottom:16px;';

        const statusRow = document.createElement('div');
        statusRow.style.cssText = 'display:flex;justify-content:space-between;align-items:center;margin-bottom:8px;';

        const permLabel = document.createElement('span');
        permLabel.innerText = '🔔 Desktop Notifications';
        permLabel.style.cssText = 'font-weight:600;font-size:14px;color:#e2e8f0;';

        const statusBadge = document.createElement('span');
        function updateBadge() {
          const state = getWarpPermission();
          if (state === 'granted') {
            statusBadge.innerText = 'Allowed';
            statusBadge.style.cssText = 'background:#166534;color:#86efac;padding:3px 10px;border-radius:6px;font-size:12px;font-weight:600;';
          } else if (state === 'denied') {
            statusBadge.innerText = 'Blocked';
            statusBadge.style.cssText = 'background:#991b1b;color:#fca5a5;padding:3px 10px;border-radius:6px;font-size:12px;font-weight:600;';
          } else {
            statusBadge.innerText = 'Ask (Default)';
            statusBadge.style.cssText = 'background:#334155;color:#94a3b8;padding:3px 10px;border-radius:6px;font-size:12px;font-weight:600;';
          }
        }
        updateBadge();

        statusRow.appendChild(permLabel);
        statusRow.appendChild(statusBadge);

        const hint = document.createElement('p');
        hint.innerText = 'Allows or blocks web and push notifications for this isolated application profile.';
        hint.style.cssText = 'font-size:12px;color:#94a3b8;margin:0 0 12px 0;line-height:1.4;';

        const btnGroup = document.createElement('div');
        btnGroup.style.cssText = 'display:flex;gap:8px;flex-wrap:wrap;';

        const allowBtn = document.createElement('button');
        allowBtn.type = 'button';
        allowBtn.innerText = '✓ Allow';
        allowBtn.style.cssText = 'padding:6px 12px;border-radius:6px;border:none;background:#22c55e;color:#ffffff;cursor:pointer;font-size:12px;font-weight:600;';
        allowBtn.onclick = function() {
          safeStorage.setItem('__warp_notification_permission', 'granted');
          notifyPermissionChanged('granted');
          updateBadge();
        };

        const blockBtn = document.createElement('button');
        blockBtn.type = 'button';
        blockBtn.innerText = '✕ Block';
        blockBtn.style.cssText = 'padding:6px 12px;border-radius:6px;border:none;background:#ef4444;color:#ffffff;cursor:pointer;font-size:12px;font-weight:600;';
        blockBtn.onclick = function() {
          safeStorage.setItem('__warp_notification_permission', 'denied');
          notifyPermissionChanged('denied');
          updateBadge();
        };

        const resetBtn = document.createElement('button');
        resetBtn.type = 'button';
        resetBtn.innerText = '↺ Reset (Ask)';
        resetBtn.style.cssText = 'padding:6px 12px;border-radius:6px;border:1px solid #475569;background:transparent;color:#94a3b8;cursor:pointer;font-size:12px;font-weight:500;';
        resetBtn.onclick = function() {
          safeStorage.setItem('__warp_notification_permission', 'default');
          notifyPermissionChanged('default');
          updateBadge();
        };

        const resetReloadBtn = document.createElement('button');
        resetReloadBtn.type = 'button';
        resetReloadBtn.innerText = '↺ Reset & Reload (F5)';
        resetReloadBtn.style.cssText = 'padding:6px 12px;border-radius:6px;border:1px solid #3b82f6;background:rgba(59,130,246,0.15);color:#93c5fd;cursor:pointer;font-size:12px;font-weight:500;';
        resetReloadBtn.onclick = function() {
          safeStorage.setItem('__warp_notification_permission', 'default');
          notifyPermissionChanged('default');
          window.location.reload();
        };

        btnGroup.appendChild(allowBtn);
        btnGroup.appendChild(blockBtn);
        btnGroup.appendChild(resetBtn);
        btnGroup.appendChild(resetReloadBtn);

        row.appendChild(statusRow);
        row.appendChild(hint);
        row.appendChild(btnGroup);

        const footerRow = document.createElement('div');
        footerRow.style.cssText = 'display:flex;justify-content:flex-end;margin-top:20px;';

        const closeBtn = document.createElement('button');
        closeBtn.type = 'button';
        closeBtn.innerText = 'Done';
        closeBtn.style.cssText = 'padding:8px 20px;border-radius:6px;border:none;background:#3b82f6;color:#ffffff;cursor:pointer;font-size:13px;font-weight:600;';

        function close() {
          if (overlay.parentNode) overlay.parentNode.removeChild(overlay);
        }

        closeBtn.onclick = close;
        overlay.onclick = function(e) {
          if (e.target === overlay) close();
        };

        box.onkeydown = function(e) {
          if (e.key === 'Escape') {
            e.preventDefault();
            close();
          }
        };

        footerRow.appendChild(closeBtn);
        box.appendChild(title);
        box.appendChild(row);
        box.appendChild(footerRow);
        overlay.appendChild(box);
        (document.body || document.documentElement).appendChild(overlay);
        closeBtn.focus();
      }

      function handleRequestPermission(callback) {
        const perm = getWarpPermission();
        if (perm === 'denied') {
          if (typeof callback === 'function') {
            try { callback('denied'); } catch (e) {}
          }
          return Promise.resolve('denied');
        }

        // Auto approve immediately
        safeStorage.setItem('__warp_notification_permission', 'granted');
        notifyPermissionChanged('granted');
        if (typeof callback === 'function') {
          try { callback('granted'); } catch (e) {}
        }
        return Promise.resolve('granted');
      }

      try {
        const OrigNotification = window.Notification;
        function WarpNotification(title, options) {
          if (!(this instanceof WarpNotification)) {
            throw new TypeError("Failed to construct 'Notification': Please use the 'new' operator.");
          }
          let instance;
          try {
            instance = new OrigNotification(title, options);
          } catch (e) {
            instance = this;
            instance.title = title;
            instance.options = options || {};
          }
          const perm = getWarpPermission();
          if (perm === 'granted') {
            showNotificationToast(title, options);
          }
          return instance;
        }

        WarpNotification.prototype = OrigNotification ? OrigNotification.prototype : {};
        Object.defineProperty(WarpNotification, 'permission', {
          get: getWarpPermission,
          configurable: true,
          enumerable: true
        });
        WarpNotification.requestPermission = handleRequestPermission;
        if (OrigNotification && OrigNotification.maxActions) {
          WarpNotification.maxActions = OrigNotification.maxActions;
        }

        if (OrigNotification) {
          try {
            Object.defineProperty(OrigNotification, 'permission', {
              get: getWarpPermission,
              configurable: true,
              enumerable: true
            });
          } catch (e) {}
          OrigNotification.requestPermission = handleRequestPermission;
        }

        window.Notification = WarpNotification;
        try {
          Notification = WarpNotification;
        } catch (e) {}
      } catch (e) {}

      // ServiceWorker Push & Notification Interception
      function createMockSubscription(options) {
        const p256dh = new Uint8Array([4, 15, 34, 52, 90, 12, 45, 67, 89, 100, 111, 122, 133, 144, 155, 166]);
        const auth = new Uint8Array([23, 45, 67, 89, 12, 34, 56, 78, 90, 11, 22, 33, 44, 55, 66, 77]);
        return {
          endpoint: 'https://warp.local/push/' + Math.random().toString(36).substring(2),
          expirationTime: null,
          options: options || {},
          getKey: function(name) {
            if (name === 'p256dh') return p256dh.buffer;
            if (name === 'auth') return auth.buffer;
            return null;
          },
          toJSON: function() {
            return {
              endpoint: this.endpoint,
              expirationTime: null,
              keys: {
                p256dh: 'BA8vIlJadAxDR1ljZW9yeYVqug==',
                auth: 'F01HVwwyVn5aCyIzRFV2cw=='
              }
            };
          },
          unsubscribe: function() {
            return Promise.resolve(true);
          }
        };
      }

      function MockPushManager() {}
      MockPushManager.prototype.subscribe = function(options) {
        const perm = getWarpPermission();
        if (perm === 'denied') {
          return Promise.reject(new DOMException('Registration failed - permission denied', 'NotAllowedError'));
        }
        safeStorage.setItem('__warp_notification_permission', 'granted');
        notifyPermissionChanged('granted');
        return Promise.resolve(createMockSubscription(options));
      };
      MockPushManager.prototype.getSubscription = function() {
        const perm = getWarpPermission();
        if (perm === 'granted') {
          return Promise.resolve(createMockSubscription());
        }
        return Promise.resolve(null);
      };
      MockPushManager.prototype.permissionState = function() {
        const perm = getWarpPermission();
        return Promise.resolve((perm === 'granted') ? 'granted' : ((perm === 'denied') ? 'denied' : 'prompt'));
      };

      try {
        window.PushManager = MockPushManager;
      } catch (e) {}

      function patchRegistration(reg) {
        if (!reg) return;
        try {
          if (!reg.pushManager) {
            reg.pushManager = new MockPushManager();
          }
        } catch (e) {}
        try {
          if (!reg.showNotification) {
            reg.showNotification = function(title, options) {
              const perm = getWarpPermission();
              if (perm === 'granted') {
                showNotificationToast(title, options);
                return Promise.resolve();
              }
              return Promise.reject(new DOMException('Notification permission denied', 'NotAllowedError'));
            };
          }
        } catch (e) {}
      }

      try {
        if (typeof ServiceWorkerRegistration !== 'undefined' && ServiceWorkerRegistration.prototype) {
          try {
            Object.defineProperty(ServiceWorkerRegistration.prototype, 'pushManager', {
              get: function() {
                if (!this.__warp_pushManager) {
                  this.__warp_pushManager = new MockPushManager();
                }
                return this.__warp_pushManager;
              },
              configurable: true,
              enumerable: true
            });
          } catch (e) {
            ServiceWorkerRegistration.prototype.pushManager = new MockPushManager();
          }

          ServiceWorkerRegistration.prototype.showNotification = function(title, options) {
            const perm = getWarpPermission();
            if (perm === 'granted') {
              showNotificationToast(title, options);
              return Promise.resolve();
            }
            return Promise.reject(new DOMException('Notification permission denied', 'NotAllowedError'));
          };
        }
      } catch (e) {}

      try {
        if (navigator.serviceWorker) {
          const origReady = navigator.serviceWorker.ready;
          if (origReady) {
            try {
              Object.defineProperty(navigator.serviceWorker, 'ready', {
                get: function() {
                  return origReady.then(function(reg) {
                    patchRegistration(reg);
                    return reg;
                  });
                },
                configurable: true
              });
            } catch (e) {}
          }

          const origRegister = navigator.serviceWorker.register;
          if (origRegister) {
            navigator.serviceWorker.register = function() {
              return origRegister.apply(this, arguments).then(function(reg) {
                patchRegistration(reg);
                return reg;
              });
            };
          }

          const origGetReg = navigator.serviceWorker.getRegistration;
          if (origGetReg) {
            navigator.serviceWorker.getRegistration = function() {
              return origGetReg.apply(this, arguments).then(function(reg) {
                patchRegistration(reg);
                return reg;
              });
            };
          }

          const origGetRegs = navigator.serviceWorker.getRegistrations;
          if (origGetRegs) {
            navigator.serviceWorker.getRegistrations = function() {
              return origGetRegs.apply(this, arguments).then(function(regs) {
                if (Array.isArray(regs)) {
                  regs.forEach(patchRegistration);
                }
                return regs;
              });
            };
          }
        }
      } catch (e) {}

      window.addEventListener('keydown', function(e) {
        // F5 or Ctrl+R: Reload
        if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r')) {
          e.preventDefault();
          window.location.reload();
          return;
        }

        // F2 or Alt + T : Open Change Window Title dialog
        if (e.key === 'F2' || (e.altKey && e.key.toLowerCase() === 't')) {
          e.preventDefault();
          showTitleDialog();
          return;
        }

        // F8 : Open Site Permissions dialog
        if (e.key === 'F8') {
          e.preventDefault();
          showPermissionsDialog();
          return;
        }

        // Ctrl + L or F6 : Open URL dialog
        if (((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') || e.key === 'F6') {
          e.preventDefault();
          showUrlDialog();
          return;
        }

        // Ctrl + Plus / '=' : Zoom In
        if ((e.ctrlKey || e.metaKey) && (e.key === '+' || e.key === '=')) {
          e.preventDefault();
          currentZoom = Math.min(currentZoom + 0.1, 3.0);
          try { document.documentElement.style.zoom = currentZoom; } catch (e) {}
          safeStorage.setItem('__warp_zoom', currentZoom.toString());
          return;
        }

        // Ctrl + Minus : Zoom Out
        if ((e.ctrlKey || e.metaKey) && (e.key === '-' || e.key === '_')) {
          e.preventDefault();
          currentZoom = Math.max(currentZoom - 0.1, 0.4);
          try { document.documentElement.style.zoom = currentZoom; } catch (e) {}
          safeStorage.setItem('__warp_zoom', currentZoom.toString());
          return;
        }

        // Ctrl + 0 : Reset Zoom
        if ((e.ctrlKey || e.metaKey) && e.key === '0') {
          e.preventDefault();
          currentZoom = 1.0;
          try { document.documentElement.style.zoom = '1.0'; } catch (e) {}
          safeStorage.removeItem('__warp_zoom');
          return;
        }

        // Alt + LeftArrow : Back
        if (e.altKey && e.key === 'ArrowLeft') {
          e.preventDefault();
          window.history.back();
          return;
        }

        // Alt + RightArrow : Forward
        if (e.altKey && e.key === 'ArrowRight') {
          e.preventDefault();
          window.history.forward();
          return;
        }
      }, true);
    })();
    "#;

    let initial_notif_perm = app.notifications.as_deref().unwrap_or("default");
    let script_init = format!(
        r#"
        (function() {{
            window.__WARP_INITIAL_NOTIF_PERM__ = "{}";
        }})();
        "#,
        initial_notif_perm
    );
    let final_script = format!("{}\n{}", script_init, shortcut_script);

    let base_host = parsed_url.host_str().map(|h| h.to_lowercase());
    let nav_base_host = base_host.clone();
    let new_win_base_host = base_host.clone();

    let builder = WebviewWindowBuilder::new(
        app_handle,
        &window_label,
        WebviewUrl::External(parsed_url),
    )
    .title(&app.name)
    .inner_size(1200.0, 800.0)
    .data_directory(profile_dir)
    .initialization_script(&final_script)
    .on_permission_request(move |_webview, kind| {
        match kind {
            tauri::webview::PermissionKind::Notifications
            | tauri::webview::PermissionKind::Autoplay => {
                tauri::webview::PermissionResponse::Allow
            }
            _ => tauri::webview::PermissionResponse::Default,
        }
    })
    .on_document_title_changed(|window, title| {
        let _ = window.set_title(&title);
    })
    .on_navigation(move |nav_url| {
        if !is_internal_url(nav_url, nav_base_host.as_deref()) {
            open_in_default_browser(nav_url.as_str());
            return false;
        }
        true
    })
    .on_new_window(move |new_url, _features| {
        if !is_internal_url(&new_url, new_win_base_host.as_deref()) {
            open_in_default_browser(new_url.as_str());
            NewWindowResponse::Deny
        } else {
            NewWindowResponse::Allow
        }
    });

    let win = builder.build().map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::{SettingsExt, WebViewExt};
        let _ = win.with_webview(|webview| {
            if let Some(settings) = webview.inner().settings() {
                settings.set_media_playback_requires_user_gesture(false);
                settings.set_media_playback_allows_inline(true);
                settings.set_enable_webaudio(true);
                settings.set_enable_media(true);
            }
        });
    }

    Ok(())

}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // CLI arguments parsing for direct PWA launch: e.g. warp --app <folder_id>
            let args: Vec<String> = std::env::args().collect();
            let mut direct_app_id: Option<String> = None;

            let mut i = 1;
            while i < args.len() {
                if args[i] == "--app" && i + 1 < args.len() {
                    direct_app_id = Some(args[i + 1].clone());
                    break;
                }
                i += 1;
            }

            if let Some(app_id) = direct_app_id {
                let apps = read_apps().unwrap_or_default();
                if let Some(target_app) = apps.into_iter().find(|a| a.id == app_id) {
                    let _ = open_pwa_window(app.handle(), &target_app);
                    // Close the default launcher window if opened
                    if let Some(main_win) = app.get_webview_window("main") {
                        let _ = main_win.close();
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_apps,
            save_app,
            delete_app,
            launch_app,
            create_desktop_shortcut,
            reorder_apps,
            set_app_notification_permission
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
