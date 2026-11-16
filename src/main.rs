#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod calculator;
mod isolation;
mod apps;
mod storage;
mod terminal;
mod trash;
mod unicode;
mod unikey;
mod wifi;
mod security_tools;
mod windows_process;

fn main() -> eframe::Result {
    let paths = match storage::AppPaths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            let message = format!("NOVA could not initialize its data folders.\n\n{error}\n\nCheck that the USB drive is writable and try again.");
            #[cfg(windows)]
            show_error_message(&message);
            #[cfg(not(windows))]
            eprintln!("{message}");
            return Err(eframe::Error::AppCreation(Box::new(error)));
        }
    };

    if let Err(error) = wifi::cleanup_stale_temp_profiles() {
        eprintln!("NOVA: could not clean stale temporary WLAN profiles: {error}");
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([800.0, 500.0])
            .with_title("NOVA OS"),
        renderer: eframe::Renderer::Glow,
        system_font_fallback: true,
        ..Default::default()
    };

    let result = eframe::run_native(
        "NOVA OS",
        options,
        Box::new(move |cc| Ok(Box::new(app::NovaApp::new(cc, paths)))),
    );
    if let Err(error) = result {
        #[cfg(windows)]
        show_error_message(&format!("NOVA could not start.\n\n{error:?}\n\nCheck the graphics driver and Windows compatibility.") );
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
fn show_error_message(message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let text: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();
    let title: Vec<u16> = OsStr::new("NOVA OS — Startup error").encode_wide().chain(Some(0)).collect();
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    // A message box is needed because release builds intentionally have no console window.
    unsafe {
        let _ = MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
    }
}
