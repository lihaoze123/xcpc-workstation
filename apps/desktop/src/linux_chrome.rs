//! Linux GTK4 titlebar polish.
//!
//! NSH's Linux host overlays native `gtk::WindowControls` (`Start` and `End`) when
//! `titleBar.style = "hidden"`. On Linux desktops without an explicit
//! `gtk-decoration-layout`, GTK4 defaults to `"menu:minimize,maximize,close"`, which
//! renders a misaligned 16px application-menu icon in the top-left corner once the
//! app's hicolor icon is installed.
//!
//! This module strips `"menu"` and `"icon"` tokens from the process-local `GtkSettings`
//! `gtk-decoration-layout` on the GTK main loop while preserving all window buttons
//! (`minimize`, `maximize`, `close`) and whichever side the user prefers them on.
//! Symbols are resolved lazily via `dlsym(RTLD_DEFAULT, ...)` so headless test runners
//! and non-GTK binaries incur zero extra link dependencies.

#[cfg(target_os = "linux")]
use std::ffi::{CStr, CString, c_char, c_int, c_void};
#[cfg(target_os = "linux")]
use std::sync::Once;

/// Removes `"menu"` and `"icon"` items from a GTK `gtk-decoration-layout` string while
/// preserving button order and the `:` left/right split. Returns `None` if nothing
/// changed.
pub(crate) fn strip_menu_from_decoration_layout(layout: &str) -> Option<String> {
    let filter_side = |side: &str| -> String {
        side.split(',')
            .map(str::trim)
            .filter(|tok| !tok.is_empty() && *tok != "menu" && *tok != "icon")
            .collect::<Vec<_>>()
            .join(",")
    };
    let cleaned = if let Some((left, right)) = layout.split_once(':') {
        format!("{}:{}", filter_side(left), filter_side(right))
    } else {
        filter_side(layout)
    };
    (cleaned != layout).then_some(cleaned)
}

#[cfg(target_os = "linux")]
pub(crate) fn strip_titlebar_icon_once() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        schedule_gtk_layout_strip();
    });
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn strip_titlebar_icon_once() {}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[cfg(target_os = "linux")]
type GSourceFunc = unsafe extern "C" fn(*mut c_void) -> c_int;
#[cfg(target_os = "linux")]
type GIdleAddFn = unsafe extern "C" fn(GSourceFunc, *mut c_void) -> u32;
#[cfg(target_os = "linux")]
type GtkSettingsGetDefaultFn = unsafe extern "C" fn() -> *mut c_void;
#[cfg(target_os = "linux")]
type GObjectGetStrFn =
    unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char, *const c_void);
#[cfg(target_os = "linux")]
type GObjectSetStrFn =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_void);
#[cfg(target_os = "linux")]
type GFreeFn = unsafe extern "C" fn(*mut c_void);

#[cfg(target_os = "linux")]
unsafe fn resolve_sym(name: &CStr) -> *mut c_void {
    // RTLD_DEFAULT is NULL (0) on glibc/musl Linux.
    unsafe { dlsym(std::ptr::null_mut(), name.as_ptr()) }
}

#[cfg(target_os = "linux")]
unsafe fn schedule_gtk_layout_strip() {
    let idle_ptr = unsafe { resolve_sym(c"g_idle_add") };
    let settings_ptr = unsafe { resolve_sym(c"gtk_settings_get_default") };
    if idle_ptr.is_null() || settings_ptr.is_null() {
        return;
    }
    let g_idle_add: GIdleAddFn = unsafe { std::mem::transmute(idle_ptr) };
    unsafe {
        g_idle_add(apply_on_gtk_main_thread, std::ptr::null_mut());
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" fn apply_on_gtk_main_thread(_data: *mut c_void) -> c_int {
    const G_SOURCE_REMOVE: c_int = 0;
    let settings_ptr = unsafe { resolve_sym(c"gtk_settings_get_default") };
    let get_ptr = unsafe { resolve_sym(c"g_object_get") };
    let set_ptr = unsafe { resolve_sym(c"g_object_set") };
    let free_ptr = unsafe { resolve_sym(c"g_free") };
    if settings_ptr.is_null() || get_ptr.is_null() || set_ptr.is_null() || free_ptr.is_null() {
        return G_SOURCE_REMOVE;
    }

    let gtk_settings_get_default: GtkSettingsGetDefaultFn =
        unsafe { std::mem::transmute(settings_ptr) };
    let g_object_get: GObjectGetStrFn = unsafe { std::mem::transmute(get_ptr) };
    let g_object_set: GObjectSetStrFn = unsafe { std::mem::transmute(set_ptr) };
    let g_free: GFreeFn = unsafe { std::mem::transmute(free_ptr) };

    let settings = unsafe { gtk_settings_get_default() };
    if settings.is_null() {
        return G_SOURCE_REMOVE;
    }

    let mut current_ptr: *mut c_char = std::ptr::null_mut();
    unsafe {
        g_object_get(
            settings,
            c"gtk-decoration-layout".as_ptr(),
            &mut current_ptr,
            std::ptr::null(),
        );
    }

    let current = if current_ptr.is_null() {
        "menu:minimize,maximize,close".to_owned()
    } else {
        let s = unsafe { CStr::from_ptr(current_ptr) }
            .to_string_lossy()
            .into_owned();
        unsafe { g_free(current_ptr.cast()) };
        s
    };

    if let Some(cleaned) = strip_menu_from_decoration_layout(&current)
        && let Ok(c_cleaned) = CString::new(cleaned)
    {
        unsafe {
            g_object_set(
                settings,
                c"gtk-decoration-layout".as_ptr(),
                c_cleaned.as_ptr(),
                std::ptr::null(),
            );
        }
    }

    G_SOURCE_REMOVE
}

#[cfg(test)]
mod tests {
    use super::strip_menu_from_decoration_layout;

    #[test]
    fn strips_menu_and_icon_while_keeping_window_buttons() {
        assert_eq!(
            strip_menu_from_decoration_layout("menu:minimize,maximize,close").as_deref(),
            Some(":minimize,maximize,close")
        );
        assert_eq!(
            strip_menu_from_decoration_layout("icon,menu:minimize,maximize,close").as_deref(),
            Some(":minimize,maximize,close")
        );
        assert_eq!(
            strip_menu_from_decoration_layout("close,minimize,maximize:menu").as_deref(),
            Some("close,minimize,maximize:")
        );
        assert_eq!(
            strip_menu_from_decoration_layout(":minimize,maximize,close"),
            None
        );
    }
}
