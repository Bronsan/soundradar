//! Native always-on-top click-through hit overlay (layered window).

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct HitCard {
    pub name: String,
    pub score: f32,
    pub born: Instant,
}

struct OverlayState {
    hits: Vec<HitCard>,
    x: i32,
    y: i32,
    size: u32,
    opacity: f32,
    duration: Duration,
}

static RUNNING: AtomicBool = AtomicBool::new(false);

fn state() -> &'static Mutex<OverlayState> {
    static S: OnceLock<Mutex<OverlayState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(OverlayState {
            hits: Vec::new(),
            x: 0,
            y: 0,
            size: 220,
            opacity: 0.9,
            duration: Duration::from_millis(2500),
        })
    })
}

pub fn set_geometry(x: i32, y: i32, size: u32, opacity: f32) {
    let mut g = state().lock().unwrap();
    if x != 0 || y != 0 {
        g.x = x;
        g.y = y;
    }
    if size > 0 {
        g.size = size.clamp(140, 480);
    }
    if opacity > 0.0 {
        g.opacity = opacity.clamp(0.3, 1.0);
    }
}

pub fn show_hit(name: &str, score: f32, _icon: Option<&[u8]>) -> Result<()> {
    let mut g = state().lock().unwrap();
    g.hits.insert(
        0,
        HitCard {
            name: name.to_string(),
            score,
            born: Instant::now(),
        },
    );
    g.hits.truncate(4);
    Ok(())
}

/// Start overlay window thread. Safe to call once; later calls no-op.
pub fn spawn_overlay_thread() -> Result<()> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    std::thread::Builder::new()
        .name("sr-overlay".into())
        .spawn(move || {
            if let Err(e) = overlay_main() {
                eprintln!("[overlay] {:#}", e);
            }
            RUNNING.store(false, Ordering::SeqCst);
        })?;
    // give the window a moment to appear
    std::thread::sleep(Duration::from_millis(150));
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn overlay_main() -> Result<()> {
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
        GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, HGDIOBJ,
        PAINTSTRUCT, TRANSPARENT, DEFAULT_CHARSET, FW_BOLD, FW_NORMAL,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, LoadCursorW, PeekMessageW,
        PostQuitMessage, RegisterClassW, SetLayeredWindowAttributes, SetTimer, SetWindowPos,
        ShowWindow, SystemParametersInfoW, TranslateMessage, CS_HREDRAW, CS_VREDRAW, HTTRANSPARENT,
        IDC_ARROW, LWA_ALPHA, LWA_COLORKEY, MSG, PM_REMOVE, SPI_GETWORKAREA, SWP_NOACTIVATE,
        SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
        WM_DESTROY, WM_NCHITTEST, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE, HWND_TOPMOST,
    };
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let class_name = wide("SoundRadarOverlay");
        let title = wide("SoundRadar");
        let font_name = wide("Microsoft YaHei UI");
        let hinst = GetModuleHandleW(None)?;

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: windows::Win32::Foundation::HINSTANCE(hinst.0),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: CreateSolidBrush(COLORREF(0x0014171c)),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );

        let (cfg_x, cfg_y, sz, op) = {
            let g = state().lock().unwrap();
            (g.x, g.y, g.size, g.opacity)
        };
        let wnd_w = sz as i32 + 32;
        let wnd_h = (sz as i32) / 2 + 80;
        let (ox, oy) = if cfg_x != 0 || cfg_y != 0 {
            (cfg_x, cfg_y)
        } else {
            (work.right - wnd_w - 24, work.bottom - wnd_h - 24)
        };

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            windows::core::PCWSTR(class_name.as_ptr()),
            windows::core::PCWSTR(title.as_ptr()),
            WS_POPUP | WS_VISIBLE,
            ox,
            oy,
            wnd_w,
            wnd_h,
            None,
            None,
            windows::Win32::Foundation::HINSTANCE(hinst.0),
            None,
        )?;

        let _ = SetLayeredWindowAttributes(
            hwnd,
            COLORREF(0x0014171c),
            (op * 255.0) as u8,
            LWA_ALPHA | LWA_COLORKEY,
        );
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            ox,
            oy,
            wnd_w,
            wnd_h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, 1, 50, None);

        eprintln!("[overlay] 悬浮窗已显示 @ ({}, {}) {}x{}", ox, oy, wnd_w, wnd_h);
        let _ = font_name;
        let _ = to_w_unused();

        let mut msg = MSG::default();
        loop {
            {
                let mut g = state().lock().unwrap();
                let dur = g.duration;
                g.hits.retain(|h| h.born.elapsed() < dur);
            }
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_DESTROY {
                    return Ok(());
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            std::thread::sleep(Duration::from_millis(33));
        }
    }
}

fn to_w_unused() {}

unsafe extern "system" fn wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{COLORREF, LRESULT};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateFontW, DeleteObject, EndPaint, FillRect,
        GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TextOutW, HGDIOBJ, PAINTSTRUCT,
        TRANSPARENT, DEFAULT_CHARSET, FW_BOLD, FW_NORMAL,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, GetClientRect, PostQuitMessage, HTTRANSPARENT, WM_DESTROY, WM_NCHITTEST,
        WM_PAINT, WM_TIMER,
    };

    match msg {
        WM_NCHITTEST => return LRESULT(-1),
        WM_DESTROY => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        WM_PAINT | WM_TIMER => {
            use windows::Win32::Graphics::Gdi::CreateSolidBrush;
            let mut ps = PAINTSTRUCT::default();
            let hdc = if msg == WM_PAINT {
                BeginPaint(hwnd, &mut ps)
            } else {
                GetDC(hwnd)
            };
            let mut rc = RECT_DEFAULT();
            let _ = GetClientRect(hwnd, &mut rc);
            let brush = CreateSolidBrush(COLORREF(0x0014171c));
            FillRect(hdc, &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));

            let hits = state().lock().unwrap().hits.clone();
            SetBkMode(hdc, TRANSPARENT);
            let mut y = 12i32;

            let (head, head_col) = if hits.is_empty() {
                ("SoundRadar  ·  待机".to_string(), COLORREF(0x00808080))
            } else {
                ("SoundRadar  ·  命中".to_string(), COLORREF(0x0080ff80))
            };
            draw_text(hdc, 12, y, &head, 14, head_col, false);
            y += 26;

            if hits.is_empty() {
                draw_text(hdc, 12, y, "等待音效…", 15, COLORREF(0x00666666), false);
            } else {
                for h in hits.iter().take(3) {
                    let line = format!("{:.2}   {}", h.score, h.name);
                    let col = if h.score >= 0.75 {
                        COLORREF(0x0080ff80)
                    } else if h.score >= 0.5 {
                        COLORREF(0x00ffff80)
                    } else {
                        COLORREF(0x00cccccc)
                    };
                    draw_text(hdc, 12, y, &line, 16, col, true);
                    y += 26;
                }
            }

            if msg == WM_PAINT {
                EndPaint(hwnd, &ps);
            } else {
                let _ = ReleaseDC(hwnd, hdc);
            }
            return LRESULT(0);
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn RECT_DEFAULT() -> windows::Win32::Foundation::RECT {
    windows::Win32::Foundation::RECT::default()
}

fn draw_text(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    x: i32,
    y: i32,
    s: &str,
    px: i32,
    color: windows::Win32::Foundation::COLORREF,
    bold: bool,
) {
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::{
        CreateFontW, DeleteObject, SelectObject, SetTextColor, TextOutW, HGDIOBJ,
        DEFAULT_CHARSET, FW_BOLD, FW_NORMAL,
    };
    unsafe {
        let font_name = wide("Microsoft YaHei UI");
        let t = wide(s);
        SetTextColor(hdc, color);
        let weight = if bold { FW_BOLD } else { FW_NORMAL };
        let font = CreateFontW(
            px,
            0,
            0,
            0,
            weight.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET.0 as u32,
            0,
            0,
            5,
            0,
            windows::core::PCWSTR(font_name.as_ptr()),
        );
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let _ = TextOutW(hdc, x, y, &t[..t.len().saturating_sub(1)]);
        SelectObject(hdc, old);
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
}
