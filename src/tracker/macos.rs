use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use objc2::rc::autoreleasepool;
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSDictionary, NSString};
use std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *mut c_void;
    fn AXUIElementCreateSystemWide() -> *mut c_void;
    fn AXUIElementCopyAttributeValue(
        element: *mut c_void,
        attribute: *const c_void,
        value: *mut *mut c_void,
    ) -> i32;
    fn AXUIElementGetPid(element: *mut c_void, pid: *mut i32) -> i32;
    fn AXUIElementSetMessagingTimeout(element: *mut c_void, timeout: f32) -> i32;
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *const c_void;
}

const ON_SCREEN_ONLY: u32 = 1;
const EXCLUDE_DESKTOP_ELEMENTS: u32 = 16;
const AX_TIMEOUT_SECONDS: f32 = 0.5;

pub struct FrontApp {
    pub name: String,
    pub bundle_id: String,
    pub path: Option<String>,
    pub pid: i32,
    pub title: Option<String>,
}

pub fn frontmost() -> Option<FrontApp> {
    let trusted = ax_trusted();
    let window_owner = front_window_owner();

    // NSWorkspace::frontmostApplication 在非 GUI 进程中会缓存启动时的应用，
    // 因此以 AX 聚焦应用为准，未授权时回退到 WindowServer 的前台窗口。
    let pid = if trusted {
        ax_focused_pid().or_else(|| window_owner.as_ref().map(|(pid, _)| *pid))
    } else {
        window_owner.as_ref().map(|(pid, _)| *pid)
    }?;

    let owner_name = window_owner
        .filter(|(owner_pid, _)| *owner_pid == pid)
        .map(|(_, name)| name)
        .unwrap_or_default();
    let (name, bundle_id, path) = resolve_application(pid, owner_name);
    let title = if trusted { window_title(pid) } else { None };

    Some(FrontApp {
        name,
        bundle_id,
        path,
        pid,
        title,
    })
}

pub fn ax_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn ax_trusted_with_prompt() -> bool {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

pub fn launch_terminal() -> Option<String> {
    let bundle = std::env::var("__CFBundleIdentifier")
        .ok()
        .filter(|value| !value.is_empty())?;
    Some(match bundle.as_str() {
        "com.apple.Terminal" => "终端".to_string(),
        "com.googlecode.iterm2" => "iTerm".to_string(),
        "com.mitchellh.ghostty" => "Ghostty".to_string(),
        "com.qoder.app" => "Qoder".to_string(),
        "dev.zed.Zed" => "Zed".to_string(),
        "com.microsoft.VSCode" => "VS Code".to_string(),
        "com.microsoft.VSCodeInsiders" => "VS Code Insiders".to_string(),
        "dev.warp.Warp-Stable" => "Warp".to_string(),
        _ => bundle,
    })
}

pub fn launch_kind(terminal: &Option<String>) -> &'static str {
    if terminal.is_some() {
        return "terminal";
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let exe = exe.to_string_lossy();
    if exe.contains("/Cellar/") || exe.contains("/homebrew/") {
        "brew"
    } else {
        "other"
    }
}

pub fn exe_path() -> String {
    std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "oxidetime".to_string())
}

pub fn idle_seconds() -> f64 {
    unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) }
}

fn resolve_application(pid: i32, fallback_name: String) -> (String, String, Option<String>) {
    autoreleasepool(|_| {
        let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) else {
            return (fallback_name, String::new(), None);
        };
        let name = app
            .localizedName()
            .map(|name| name.to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or(fallback_name);
        let bundle_id = app
            .bundleIdentifier()
            .map(|id| id.to_string())
            .unwrap_or_default();
        let path = app
            .executableURL()
            .and_then(|url| url.path())
            .map(|path| path.to_string());
        (name, bundle_id, path)
    })
}

fn front_window_owner() -> Option<(i32, String)> {
    unsafe {
        let raw = CGWindowListCopyWindowInfo(ON_SCREEN_ONLY | EXCLUDE_DESKTOP_ELEMENTS, 0);
        if raw.is_null() {
            return None;
        }
        let windows: CFArray<CFDictionary<CFString, CFType>> =
            CFArray::wrap_under_create_rule(raw as *const _);

        for window in windows.iter() {
            let layer = window
                .find(&CFString::new("kCGWindowLayer"))
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i64());
            if layer != Some(0) {
                continue;
            }
            let alpha = window
                .find(&CFString::new("kCGWindowAlpha"))
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_f64())
                .unwrap_or(1.0);
            if alpha <= 0.0 {
                continue;
            }
            let Some(pid) = window
                .find(&CFString::new("kCGWindowOwnerPID"))
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|number| number.to_i64())
            else {
                continue;
            };
            if pid <= 0 {
                continue;
            }
            let name = window
                .find(&CFString::new("kCGWindowOwnerName"))
                .and_then(|value| value.downcast::<CFString>())
                .map(|name| name.to_string())
                .unwrap_or_default();
            return Some((pid as i32, name));
        }
        None
    }
}

fn ax_focused_pid() -> Option<i32> {
    let system = unsafe { AXUIElementCreateSystemWide() };
    if system.is_null() {
        return None;
    }
    let system = unsafe { CFType::wrap_under_create_rule(system as *const c_void) };
    let system_ref = system.as_CFTypeRef() as *mut c_void;
    unsafe {
        AXUIElementSetMessagingTimeout(system_ref, AX_TIMEOUT_SECONDS);
    }

    let app = copy_attr(system_ref, "AXFocusedApplication")?;
    let mut pid = 0;
    let err = unsafe { AXUIElementGetPid(app.as_CFTypeRef() as *mut c_void, &mut pid) };
    if err == 0 && pid > 0 { Some(pid) } else { None }
}

fn copy_attr(element: *mut c_void, attribute: &str) -> Option<CFType> {
    let name = CFString::new(attribute);
    let mut value: *mut c_void = std::ptr::null_mut();
    let err = unsafe {
        AXUIElementCopyAttributeValue(
            element,
            name.as_concrete_TypeRef() as *const c_void,
            &mut value,
        )
    };
    if err == 0 && !value.is_null() {
        Some(unsafe { CFType::wrap_under_create_rule(value as *const c_void) })
    } else {
        None
    }
}

fn window_title(pid: i32) -> Option<String> {
    let app = unsafe { AXUIElementCreateApplication(pid) };
    if app.is_null() {
        return None;
    }
    unsafe {
        AXUIElementSetMessagingTimeout(app, AX_TIMEOUT_SECONDS);
    }
    let app = unsafe { CFType::wrap_under_create_rule(app as *const c_void) };

    for window_attr in ["AXFocusedWindow", "AXMainWindow"] {
        let Some(window) = copy_attr(app.as_CFTypeRef() as *mut c_void, window_attr) else {
            continue;
        };
        let Some(title) = copy_attr(window.as_CFTypeRef() as *mut c_void, "AXTitle") else {
            continue;
        };
        if let Some(text) = title.downcast::<CFString>() {
            return Some(text.to_string());
        }
    }
    None
}

pub fn icon_png(path: &str) -> Option<Vec<u8>> {
    let root = app_bundle_root(path);
    autoreleasepool(|_| unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let ns_path = NSString::from_str(&root);
        let image = workspace.iconForFile(&ns_path);
        let tiff = image.TIFFRepresentation()?;
        let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
        let props = NSDictionary::new();
        let data = rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props)?;
        Some(data.to_vec())
    })
}

fn app_bundle_root(path: &str) -> String {
    match path.find(".app/") {
        Some(idx) => path[..idx + 4].to_string(),
        None => path.to_string(),
    }
}
