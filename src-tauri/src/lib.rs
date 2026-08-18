use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum DisplayKind {
    BuiltIn,
    Physical,
    Virtual,
    Unknown,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplayInfo {
    id: u32,
    kind: DisplayKind,
    built_in: bool,
    main: bool,
    active: bool,
    online: bool,
    width: usize,
    height: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplayStatus {
    displays: Vec<DisplayInfo>,
    has_built_in: bool,
    external_count: usize,
    virtual_count: usize,
    unknown_count: usize,
    can_safely_disconnect: bool,
    platform_supported: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    available: bool,
    symbol: Option<String>,
    test_running: bool,
    automation_enabled: bool,
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{DisplayInfo, DisplayKind};
    use core_foundation::base::{CFTypeRef, TCFType};
    use core_foundation::boolean::{kCFBooleanFalse, kCFBooleanTrue};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryGetValue, CFDictionaryRef};
    use core_foundation::string::CFString;
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    type CGDirectDisplayId = u32;
    type CGError = i32;
    type CGDisplayConfigRef = *mut c_void;
    type ConfigureDisplayEnabled = unsafe extern "C" fn(CGDisplayConfigRef, u32, bool) -> CGError;

    static TEST_DISPLAY: Mutex<Option<u32>> = Mutex::new(None);
    static MANAGED_DISPLAY: Mutex<Option<u32>> = Mutex::new(None);
    static WATCHDOG: Mutex<Option<std::process::Child>> = Mutex::new(None);
    static AUTOMATION_ENABLED: AtomicBool = AtomicBool::new(false);
    static DEFERRED_RESTORE: AtomicBool = AtomicBool::new(false);

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetOnlineDisplayList(
            max_displays: u32,
            online_displays: *mut CGDirectDisplayId,
            display_count: *mut u32,
        ) -> CGError;
        fn CGDisplayIsBuiltin(display: CGDirectDisplayId) -> u32;
        fn CGDisplayIsMain(display: CGDirectDisplayId) -> u32;
        fn CGDisplayIsActive(display: CGDirectDisplayId) -> u32;
        fn CGDisplayIsOnline(display: CGDirectDisplayId) -> u32;
        fn CGDisplayPixelsWide(display: CGDirectDisplayId) -> usize;
        fn CGDisplayPixelsHigh(display: CGDirectDisplayId) -> usize;
        fn CGBeginDisplayConfiguration(config: *mut CGDisplayConfigRef) -> CGError;
        fn CGCompleteDisplayConfiguration(config: CGDisplayConfigRef, option: u32) -> CGError;
        fn CGCancelDisplayConfiguration(config: CGDisplayConfigRef) -> CGError;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[link(name = "CoreDisplay", kind = "framework")]
    unsafe extern "C" {
        fn CoreDisplay_DisplayCreateInfoDictionary(display: CGDirectDisplayId) -> CFDictionaryRef;
    }

    fn display_kind(id: CGDirectDisplayId, built_in: bool) -> DisplayKind {
        if built_in {
            return DisplayKind::BuiltIn;
        }
        let dictionary_ref = unsafe { CoreDisplay_DisplayCreateInfoDictionary(id) };
        if dictionary_ref.is_null() {
            return DisplayKind::Unknown;
        }
        let dictionary: CFDictionary = unsafe { TCFType::wrap_under_create_rule(dictionary_ref) };
        let key = CFString::new("kCGDisplayIsVirtualDevice");
        let value = unsafe {
            CFDictionaryGetValue(dictionary.as_concrete_TypeRef(), key.as_CFTypeRef()) as CFTypeRef
        };
        if value == unsafe { kCFBooleanTrue } as CFTypeRef {
            DisplayKind::Virtual
        } else if value == unsafe { kCFBooleanFalse } as CFTypeRef {
            DisplayKind::Physical
        } else {
            DisplayKind::Unknown
        }
    }

    pub fn online_displays() -> Result<Vec<DisplayInfo>, String> {
        let mut count = 0;
        let result = unsafe { CGGetOnlineDisplayList(0, std::ptr::null_mut(), &mut count) };
        if result != 0 {
            return Err(format!(
                "Core Graphics could not count displays (error {result})"
            ));
        }

        let mut ids = vec![0; count as usize];
        let result = unsafe { CGGetOnlineDisplayList(count, ids.as_mut_ptr(), &mut count) };
        if result != 0 {
            return Err(format!(
                "Core Graphics could not list displays (error {result})"
            ));
        }
        ids.truncate(count as usize);

        Ok(ids
            .into_iter()
            .map(|id| {
                let built_in = unsafe { CGDisplayIsBuiltin(id) != 0 };
                DisplayInfo {
                    id,
                    kind: display_kind(id, built_in),
                    built_in,
                    main: unsafe { CGDisplayIsMain(id) != 0 },
                    active: unsafe { CGDisplayIsActive(id) != 0 },
                    online: unsafe { CGDisplayIsOnline(id) != 0 },
                    width: unsafe { CGDisplayPixelsWide(id) },
                    height: unsafe { CGDisplayPixelsHigh(id) },
                }
            })
            .collect())
    }

    fn configure_symbol() -> Option<(&'static str, ConfigureDisplayEnabled)> {
        const RTLD_DEFAULT: *mut c_void = -2_isize as *mut c_void;
        for (name, symbol) in [
            (
                "CGSConfigureDisplayEnabled",
                b"CGSConfigureDisplayEnabled\0".as_ptr(),
            ),
            (
                "SLSConfigureDisplayEnabled",
                b"SLSConfigureDisplayEnabled\0".as_ptr(),
            ),
        ] {
            let address = unsafe { dlsym(RTLD_DEFAULT, symbol.cast()) };
            if !address.is_null() {
                let function =
                    unsafe { std::mem::transmute::<*mut c_void, ConfigureDisplayEnabled>(address) };
                return Some((name, function));
            }
        }
        None
    }

    pub fn engine_status() -> (Option<String>, bool, bool) {
        let symbol = configure_symbol().map(|(name, _)| name.to_string());
        let running = TEST_DISPLAY
            .lock()
            .map(|value| value.is_some())
            .unwrap_or(false);
        (symbol, running, AUTOMATION_ENABLED.load(Ordering::SeqCst))
    }

    fn set_enabled_with_scope(display_id: u32, enabled: bool, scope: u32) -> Result<(), String> {
        let (name, configure) = configure_symbol().ok_or_else(|| {
            "The private macOS display configuration symbol is unavailable".to_string()
        })?;
        let mut config: CGDisplayConfigRef = std::ptr::null_mut();
        let begin = unsafe { CGBeginDisplayConfiguration(&mut config) };
        if begin != 0 || config.is_null() {
            return Err(format!(
                "Could not begin display configuration (error {begin})"
            ));
        }

        let configured = unsafe { configure(config, display_id, enabled) };
        if configured != 0 {
            unsafe { CGCancelDisplayConfiguration(config) };
            return Err(format!(
                "{name} rejected display {display_id} (error {configured})"
            ));
        }

        let completed = unsafe { CGCompleteDisplayConfiguration(config, scope) };
        if completed != 0 {
            return Err(format!(
                "Could not complete display configuration (error {completed})"
            ));
        }
        Ok(())
    }

    fn set_enabled(display_id: u32, enabled: bool) -> Result<(), String> {
        // App-only scope means WindowServer can unwind the change if the process disappears.
        set_enabled_with_scope(display_id, enabled, 0)
    }

    fn start_watchdog(display_id: u32) -> Result<(), String> {
        let mut watchdog = WATCHDOG
            .lock()
            .map_err(|_| "Watchdog state is unavailable")?;
        if watchdog
            .as_mut()
            .is_some_and(|child| child.try_wait().ok().flatten().is_none())
        {
            return Ok(());
        }
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let parent_pid = std::process::id().to_string();
        let display_id_arg = display_id.to_string();
        let child = std::process::Command::new(executable)
            .args([
                "--broken-screen-watchdog",
                parent_pid.as_str(),
                display_id_arg.as_str(),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start crash watchdog: {error}"))?;
        *watchdog = Some(child);
        Ok(())
    }

    fn stop_watchdog() {
        if let Ok(mut watchdog) = WATCHDOG.lock() {
            if let Some(mut child) = watchdog.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    pub fn watchdog(parent_pid: i32, display_id: u32) {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        while unsafe { kill(parent_pid, 0) } == 0 {
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        // Session scope persists after this helper exits and makes recovery explicit.
        let _ = set_enabled_with_scope(display_id, true, 1);
    }

    pub fn begin_disconnect_test(seconds: u64) -> Result<u64, String> {
        let displays = online_displays()?;
        let external_count = displays
            .iter()
            .filter(|display| {
                display.kind == DisplayKind::Physical && display.active && display.online
            })
            .count();
        if external_count == 0 {
            return Err("Connect a physical external display before running the test".into());
        }
        let internal_id = displays
            .iter()
            .find(|display| display.built_in && display.active && display.online)
            .map(|display| display.id)
            .ok_or_else(|| "The built-in display is not currently active".to_string())?;

        {
            let mut test = TEST_DISPLAY
                .lock()
                .map_err(|_| "Test state is unavailable")?;
            if test.is_some() {
                return Err("A disconnect test is already running".into());
            }
            *test = Some(internal_id);
        }

        if let Err(error) = set_enabled(internal_id, false) {
            if let Ok(mut test) = TEST_DISPLAY.lock() {
                *test = None;
            }
            return Err(error);
        }

        let duration = seconds.clamp(5, 30);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(duration));
            let _ = set_enabled(internal_id, true);
            if let Ok(mut test) = TEST_DISPLAY.lock() {
                *test = None;
            }
        });
        Ok(duration)
    }

    pub fn restore_test_display() -> Result<(), String> {
        let display_id = TEST_DISPLAY
            .lock()
            .map_err(|_| "Test state is unavailable")?
            .take();
        if let Some(display_id) = display_id {
            set_enabled(display_id, true)?;
        }
        Ok(())
    }

    fn preference_path() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(|home| {
            std::path::PathBuf::from(home)
                .join("Library/Application Support/Broken Screen for Mac/automation-enabled")
        })
    }

    fn persist_automation(enabled: bool) -> Result<(), String> {
        let path = preference_path()
            .ok_or_else(|| "Could not locate the user settings folder".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(path, if enabled { "true" } else { "false" })
            .map_err(|error| error.to_string())
    }

    fn clamshell_closed() -> Option<bool> {
        let output = std::process::Command::new("/usr/sbin/ioreg")
            .args([
                "-r",
                "-n",
                "IOPMrootDomain",
                "-d",
                "1",
                "-k",
                "AppleClamshellState",
            ])
            .output()
            .ok()?;
        let registry = String::from_utf8(output.stdout).ok()?;
        if registry.contains("\"AppleClamshellState\" = Yes") {
            Some(true)
        } else if registry.contains("\"AppleClamshellState\" = No") {
            Some(false)
        } else {
            None
        }
    }

    fn restore_managed_display(force: bool) -> Result<(), String> {
        let cached = *MANAGED_DISPLAY
            .lock()
            .map_err(|_| "Display state is unavailable")?;
        if cached.is_none() {
            DEFERRED_RESTORE.store(false, Ordering::SeqCst);
            stop_watchdog();
            return Ok(());
        }
        if !force && clamshell_closed().unwrap_or(true) {
            DEFERRED_RESTORE.store(true, Ordering::SeqCst);
            return Ok(());
        }
        let display_id = MANAGED_DISPLAY
            .lock()
            .map_err(|_| "Display state is unavailable")?
            .take();
        if let Some(display_id) = display_id {
            set_enabled(display_id, true)?;
        }
        DEFERRED_RESTORE.store(false, Ordering::SeqCst);
        stop_watchdog();
        Ok(())
    }

    pub fn reconcile() -> Result<(), String> {
        let displays = online_displays()?;
        let external_count = displays
            .iter()
            .filter(|display| {
                display.kind == DisplayKind::Physical && display.active && display.online
            })
            .count();
        let internal = displays
            .iter()
            .find(|display| display.built_in && display.online);

        if !AUTOMATION_ENABLED.load(Ordering::SeqCst) {
            return restore_managed_display(false);
        }
        if external_count == 0 {
            return restore_managed_display(true);
        }

        DEFERRED_RESTORE.store(false, Ordering::SeqCst);

        if let Some(internal) = internal {
            if let Ok(mut cached) = MANAGED_DISPLAY.lock() {
                *cached = Some(internal.id);
            }
            if internal.active {
                start_watchdog(internal.id)?;
                set_enabled(internal.id, false)?;
            }
        }
        Ok(())
    }

    pub fn set_automation(enabled: bool) -> Result<(), String> {
        if enabled && configure_symbol().is_none() {
            return Err("The private macOS display API is unavailable".into());
        }
        AUTOMATION_ENABLED.store(enabled, Ordering::SeqCst);
        if let Err(error) = persist_automation(enabled) {
            AUTOMATION_ENABLED.store(false, Ordering::SeqCst);
            return Err(error);
        }
        reconcile()
    }

    pub fn start_monitor() {
        let enabled = preference_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|value| value.trim() == "true")
            .unwrap_or(false);
        AUTOMATION_ENABLED.store(enabled, Ordering::SeqCst);
        std::thread::spawn(|| loop {
            let _ = reconcile();
            // Lid-open transitions can be brief. Keep this safety check responsive;
            // unlike the removed frontend timer, it does not repaint the window.
            std::thread::sleep(std::time::Duration::from_secs(1));
        });
    }

    pub fn shutdown() {
        AUTOMATION_ENABLED.store(false, Ordering::SeqCst);
        let _ = restore_managed_display(true);
        let _ = restore_test_display();
        stop_watchdog();
    }
}

pub fn run_watchdog_if_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--broken-screen-watchdog") {
        return false;
    }
    #[cfg(target_os = "macos")]
    if let (Some(parent), Some(display)) = (args.get(2), args.get(3)) {
        if let (Ok(parent_pid), Ok(display_id)) = (parent.parse::<i32>(), display.parse::<u32>()) {
            macos::watchdog(parent_pid, display_id);
        }
    }
    true
}

#[tauri::command]
fn display_status() -> Result<DisplayStatus, String> {
    #[cfg(target_os = "macos")]
    let displays = macos::online_displays()?;

    #[cfg(not(target_os = "macos"))]
    let displays: Vec<DisplayInfo> = Vec::new();

    let has_built_in = displays.iter().any(|display| display.built_in);
    let external_count = displays
        .iter()
        .filter(|display| display.kind == DisplayKind::Physical && display.active && display.online)
        .count();
    let virtual_count = displays
        .iter()
        .filter(|display| display.kind == DisplayKind::Virtual && display.active && display.online)
        .count();
    let unknown_count = displays
        .iter()
        .filter(|display| display.kind == DisplayKind::Unknown && display.active && display.online)
        .count();

    Ok(DisplayStatus {
        can_safely_disconnect: has_built_in && external_count > 0,
        displays,
        has_built_in,
        external_count,
        virtual_count,
        unknown_count,
        platform_supported: cfg!(target_os = "macos") && cfg!(target_arch = "aarch64"),
    })
}

#[tauri::command]
fn engine_status() -> EngineStatus {
    #[cfg(target_os = "macos")]
    let (symbol, test_running, automation_enabled) = macos::engine_status();
    #[cfg(not(target_os = "macos"))]
    let (symbol, test_running, automation_enabled) = (None, false, false);
    EngineStatus {
        available: symbol.is_some(),
        symbol,
        test_running,
        automation_enabled,
    }
}

#[tauri::command]
fn set_automation(enabled: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::set_automation(enabled);
    #[cfg(not(target_os = "macos"))]
    Err("Broken Screen automation is only available on macOS".into())
}

#[tauri::command]
fn begin_disconnect_test(seconds: u64) -> Result<u64, String> {
    #[cfg(target_os = "macos")]
    return macos::begin_disconnect_test(seconds);
    #[cfg(not(target_os = "macos"))]
    Err("Broken Screen display switching is only available on macOS".into())
}

#[tauri::command]
fn restore_test_display() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return macos::restore_test_display();
    #[cfg(not(target_os = "macos"))]
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            use tauri::Manager;

            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))?;

            #[cfg(target_os = "macos")]
            app.handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            #[cfg(target_os = "macos")]
            macos::start_monitor();

            #[cfg(desktop)]
            {
                use tauri::menu::{Menu, MenuItem};
                use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

                let open =
                    MenuItem::with_id(app, "open", "Open Broken Screen", true, None::<&str>)?;
                let toggle =
                    MenuItem::with_id(app, "toggle", "Toggle On / Off", true, None::<&str>)?;
                let quit =
                    MenuItem::with_id(app, "quit", "Quit Broken Screen", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &toggle, &quit])?;

                TrayIconBuilder::with_id("broken-screen")
                    .icon(app.default_window_icon().expect("app icon").clone())
                    .icon_as_template(true)
                    .tooltip("Broken Screen for Mac")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "open" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "toggle" => {
                            #[cfg(target_os = "macos")]
                            {
                                let (_, _, enabled) = macos::engine_status();
                                let _ = macos::set_automation(!enabled);
                            }
                        }
                        "quit" => app.exit(0),
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            }
                        ) {
                            if let Some(window) = tray.app_handle().get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            display_status,
            engine_status,
            begin_disconnect_test,
            restore_test_display,
            set_automation
        ])
        .build(tauri::generate_context!())
        .expect("error while building Broken Screen for Mac");
    app.run(|app_handle, event| {
        use tauri::Manager;
        match event {
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if label == "main" => {
                api.prevent_close();
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. } => {
                #[cfg(target_os = "macos")]
                macos::shutdown();
            }
            _ => {}
        }
    });
}
