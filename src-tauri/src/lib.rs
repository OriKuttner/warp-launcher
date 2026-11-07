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

#[tauri::command]
fn save_app(name: String, url: String, folder_id: String) -> Result<PwaApp, String> {
    let id = sanitize_id(&folder_id);
    let mut apps = read_apps()?;

    // Check if updating or adding
    let icon_path = fetch_favicon(&url, &id);

    let mut app = PwaApp {
        id: id.clone(),
        name: name.trim().to_string(),
        url: url.trim().to_string(),
        icon: icon_path,
    };

    if let Some(existing) = apps.iter_mut().find(|a| a.id == id) {
        existing.name = app.name.clone();
        existing.url = app.url.clone();
        if app.icon.is_some() {
            existing.icon = app.icon.clone();
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
fn delete_app(id: String) -> Result<(), String> {
    let mut apps = read_apps()?;
    apps.retain(|a| a.id != id);
    write_apps(&apps)?;
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

      try {
        localStorage.setItem('isInstalled', 'true');
      } catch (e) {}

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

      // 2. Restore saved zoom level
      let currentZoom = parseFloat(localStorage.getItem('__warp_zoom') || '1.0');
      if (currentZoom !== 1.0) {
        document.documentElement.style.zoom = currentZoom;
      }

      // 3. Floating URL Navigation Dialog (Ctrl + L or F6)
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

        btnRow.appendChild(cancelBtn);
        btnRow.appendChild(goBtn);
        box.appendChild(label);
        box.appendChild(input);
        box.appendChild(btnRow);
        overlay.appendChild(box);
        document.body.appendChild(overlay);

        input.focus();
        input.select();
      }

      window.addEventListener('keydown', function(e) {
        // F5 or Ctrl+R: Reload
        if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'r')) {
          e.preventDefault();
          window.location.reload();
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
          document.documentElement.style.zoom = currentZoom;
          localStorage.setItem('__warp_zoom', currentZoom.toString());
          return;
        }

        // Ctrl + Minus : Zoom Out
        if ((e.ctrlKey || e.metaKey) && (e.key === '-' || e.key === '_')) {
          e.preventDefault();
          currentZoom = Math.max(currentZoom - 0.1, 0.4);
          document.documentElement.style.zoom = currentZoom;
          localStorage.setItem('__warp_zoom', currentZoom.toString());
          return;
        }

        // Ctrl + 0 : Reset Zoom
        if ((e.ctrlKey || e.metaKey) && e.key === '0') {
          e.preventDefault();
          currentZoom = 1.0;
          document.documentElement.style.zoom = '1.0';
          localStorage.removeItem('__warp_zoom');
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
    .initialization_script(shortcut_script)
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

    builder.build().map_err(|e| e.to_string())?;
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
            create_desktop_shortcut
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
