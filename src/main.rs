#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::ffi::OsStr;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint,
    FillRect, GetMonitorInfoW, GetStockObject, InvalidateRect, LineTo, MonitorFromWindow, MoveToEx,
    RoundRect, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DT_LEFT, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, NULL_BRUSH, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, FindWindowExW, FindWindowW, GetClassNameW,
    GetForegroundWindow, GetMessageW, GetWindowRect, KillTimer, PostQuitMessage, RegisterClassW,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, HWND_TOPMOST, LWA_COLORKEY, MSG, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE,
    WM_CREATE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

// Cores no formato Win32 COLORREF (0x00BBGGRR)
const TRANSPARENT_KEY: u32 = 0x00FF00FF;
const COLOR_WHITE: u32 = 0x00FFFFFF;   // > 20% (normal)
const COLOR_YELLOW: u32 = 0x003CD2FF;  // 11% a 20% (alerta)
const COLOR_RED: u32 = 0x005555FF;     // <= 10% (crítico)

const TIMER_UI_REFRESH_ID: usize = 1;

// Dimensões do widget na barra de tarefas do Windows 11
const WIDGET_WIDTH: i32 = 76;
const WIDGET_HEIGHT: i32 = 40;

// ID exato do receptor do Delux M900 Pro 8K (LXDDZ - 2.4G 8K HS Receiver)
const DELUX_M900_VID: u16 = 0x1D57;
const DELUX_M900_PID: u16 = 0xFA65;
const TELEMETRY_USAGE_PAGE: u16 = 0x000A;
const TELEMETRY_USAGE: u16 = 0x0000;

static MOUSE_BATTERY: AtomicI32 = AtomicI32::new(-1);
static LAST_DRAWN_BATTERY: AtomicI32 = AtomicI32::new(-2);
static IS_VISIBLE: AtomicBool = AtomicBool::new(true);

fn to_wstring(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Retorna a cor do ícone e texto conforme o nível de bateria
fn battery_color(pct: i32) -> COLORREF {
    if pct < 0 {
        COLORREF(COLOR_WHITE)
    } else if pct <= 10 {
        COLORREF(COLOR_RED)
    } else if pct <= 20 {
        COLORREF(COLOR_YELLOW)
    } else {
        COLORREF(COLOR_WHITE)
    }
}

/// Filtra exclusivamente a interface de telemetria passiva `000A:0000` do receptor `1D57:FA65`
fn is_m900_battery_interface(info: &hidapi::DeviceInfo) -> bool {
    info.vendor_id() == DELUX_M900_VID
        && info.product_id() == DELUX_M900_PID
        && info.usage_page() == TELEMETRY_USAGE_PAGE
        && info.usage() == TELEMETRY_USAGE
}

/// Decodifica o pacote passivo de 5 bytes do Delux M900 Pro 8K (`03 50 41 01 <bateria>`)
fn parse_fa65_battery_packet(buf: &[u8]) -> Option<i32> {
    if buf.len() >= 5 && buf[0] == 0x03 && (buf[2] == 0x40 || buf[2] == 0x41) {
        let status = buf[3];
        let pct = buf[4] as i32;
        if status != 0x02 && status != 0x03 && (1..=100).contains(&pct) {
            return Some(pct);
        }
    }
    None
}

/// Thread única em background (somente leitura) para escutar o pacote passivo do Delux M900 Pro 8K
fn spawn_battery_worker() {
    thread::spawn(|| {
        loop {
            let mut found = false;

            if let Ok(api) = hidapi::HidApi::new() {
                if let Some(info) = api.device_list().find(|i| is_m900_battery_interface(i)) {
                    let path = info.path().to_owned();
                    if let Ok(dev) = api.open_path(&path) {
                        found = true;
                        let mut buf = [0u8; 64];
                        loop {
                            match dev.read_timeout(&mut buf, 5000) {
                                Ok(len) if len > 0 => {
                                    if let Some(pct) = parse_fa65_battery_packet(&buf[..len]) {
                                        MOUSE_BATTERY.store(pct, Ordering::Relaxed);
                                    }
                                }
                                Ok(_) => {}
                                Err(_) => break, // Dongle desconectado
                            }
                        }
                    }
                }
            }

            if !found {
                MOUSE_BATTERY.store(-1, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_secs(3));
        }
    });
}

/// Modo de diagnóstico (`cargo run -- --diagnose`)
fn run_diagnostics() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
    println!("=== Diagnóstico Passivo: Delux M900 Pro 8K (1D57:FA65 / 000A:0000) ===");

    let api = match hidapi::HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Erro ao inicializar hidapi: {e}");
            return;
        }
    };

    let Some(info) = api.device_list().find(|i| is_m900_battery_interface(i)) else {
        println!("Interface 1D57:FA65 (000A:0000) não encontrada.");
        return;
    };

    let dev = match info.open_device(&api) {
        Ok(d) => d,
        Err(e) => {
            println!("Falha ao abrir interface: {e}");
            return;
        }
    };

    let start = Instant::now();
    let mut buf = [0u8; 64];
    while start.elapsed() < Duration::from_secs(6) {
        if let Ok(len) = dev.read_timeout(&mut buf, 500) {
            if len > 0 {
                print!("Pacote ({} bytes): ", len);
                for b in &buf[..len] {
                    print!("{:02X} ", b);
                }
                if let Some(pct) = parse_fa65_battery_packet(&buf[..len]) {
                    println!(" ==> BATERIA: {}%", pct);
                } else {
                    println!();
                }
            }
        }
    }
    println!("=== Fim do diagnóstico ===");
}

/// Verifica se existe algum jogo ou vídeo em tela cheia no mesmo monitor da barra de tarefas
unsafe fn is_fullscreen_active(taskbar_hwnd: HWND) -> bool {
    let fg = GetForegroundWindow();
    if fg.0.is_null() || fg == taskbar_hwnd {
        return false;
    }

    // Ignora a Área de Trabalho (Progman / WorkerW) e a própria barra de tarefas
    let mut class_buf = [0u16; 64];
    let len = GetClassNameW(fg, &mut class_buf);
    if len > 0 {
        let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
        if matches!(
            class_name.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
    }

    // Confirma se a janela em foco está no mesmo monitor da barra de tarefas
    let fg_monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY);
    let tb_monitor = MonitorFromWindow(taskbar_hwnd, MONITOR_DEFAULTTOPRIMARY);
    if fg_monitor != tb_monitor {
        return false;
    }

    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    if !GetMonitorInfoW(fg_monitor, &mut mi).as_bool() {
        return false;
    }

    let mut fg_rect: RECT = zeroed();
    if GetWindowRect(fg, &mut fg_rect).is_err() {
        return false;
    }

    fg_rect.left <= mi.rcMonitor.left
        && fg_rect.top <= mi.rcMonitor.top
        && fg_rect.right >= mi.rcMonitor.right
        && fg_rect.bottom >= mi.rcMonitor.bottom
}

/// Posiciona o widget ao lado da bandeja ou oculta automaticamente em tela cheia
unsafe fn update_position_and_visibility(hwnd: HWND) {
    let tray_class = to_wstring("Shell_TrayWnd");
    let taskbar_hwnd = FindWindowW(PCWSTR(tray_class.as_ptr()), PCWSTR::null()).unwrap_or_default();
    if taskbar_hwnd.0.is_null() {
        return;
    }

    if is_fullscreen_active(taskbar_hwnd) {
        if IS_VISIBLE.swap(false, Ordering::Relaxed) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        return;
    }

    if !IS_VISIBLE.swap(true, Ordering::Relaxed) {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    let notify_class = to_wstring("TrayNotifyWnd");
    let notify_hwnd = FindWindowExW(
        taskbar_hwnd,
        HWND::default(),
        PCWSTR(notify_class.as_ptr()),
        PCWSTR::null(),
    )
    .unwrap_or_default();

    let mut taskbar_rect: RECT = zeroed();
    let _ = GetWindowRect(taskbar_hwnd, &mut taskbar_rect);

    let (target_x, target_y) = if !notify_hwnd.0.is_null() {
        let mut notify_rect: RECT = zeroed();
        let _ = GetWindowRect(notify_hwnd, &mut notify_rect);
        let height = taskbar_rect.bottom - taskbar_rect.top;
        let y = taskbar_rect.top + (height - WIDGET_HEIGHT) / 2;
        (notify_rect.left - WIDGET_WIDTH - 6, y)
    } else {
        (taskbar_rect.right - 220, taskbar_rect.top + 4)
    };

    let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        target_x,
        target_y,
        WIDGET_WIDTH,
        WIDGET_HEIGHT,
        SWP_NOACTIVATE,
    );
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            update_position_and_visibility(hwnd);
            // Verifica tela cheia / alinhamento a cada 500ms (custo de CPU ~0%)
            let _ = SetTimer(hwnd, TIMER_UI_REFRESH_ID, 500, None);
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_UI_REFRESH_ID {
                update_position_and_visibility(hwnd);
                let current = MOUSE_BATTERY.load(Ordering::Relaxed);
                if IS_VISIBLE.load(Ordering::Relaxed)
                    && current != LAST_DRAWN_BATTERY.load(Ordering::Relaxed)
                {
                    let _ = InvalidateRect(hwnd, None, true);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);

            // Fundo transparente (ColorKey)
            let bg_brush = CreateSolidBrush(COLORREF(TRANSPARENT_KEY));
            FillRect(hdc, &ps.rcPaint, bg_brush);
            let _ = DeleteObject(bg_brush);

            let pct = MOUSE_BATTERY.load(Ordering::Relaxed);
            LAST_DRAWN_BATTERY.store(pct, Ordering::Relaxed);
            let fg_color = battery_color(pct);

            // Desenha ícone minimalista de mouse na cor dinâmica (branco / amarelo / vermelho)
            let pen = CreatePen(PS_SOLID, 2, fg_color);
            let old_pen = SelectObject(hdc, pen);
            let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));

            let icon_left = 4;
            let icon_top = (WIDGET_HEIGHT - 19) / 2;
            let icon_right = icon_left + 13;
            let icon_bottom = icon_top + 19;
            let _ = RoundRect(hdc, icon_left, icon_top, icon_right, icon_bottom, 10, 10);

            // Linha do scroll wheel no topo do mouse
            let scroll_x = (icon_left + icon_right) / 2;
            let _ = MoveToEx(hdc, scroll_x, icon_top + 4, None);
            let _ = LineTo(hdc, scroll_x, icon_top + 9);

            SelectObject(hdc, old_brush);
            SelectObject(hdc, old_pen);
            let _ = DeleteObject(pen);

            // Texto da porcentagem em Segoe UI Variable Display
            let font_name = to_wstring("Segoe UI Variable Display");
            let hfont = CreateFontW(
                18,
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.0 as u32,
                OUT_DEFAULT_PRECIS.0 as u32,
                CLIP_DEFAULT_PRECIS.0 as u32,
                CLEARTYPE_QUALITY.0 as u32,
                0,
                PCWSTR(font_name.as_ptr()),
            );
            let old_font = SelectObject(hdc, hfont);

            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, fg_color);

            let label = if pct >= 0 {
                format!("{}%", pct)
            } else {
                "--%".to_string()
            };

            let mut text_wide = to_wstring(&label);
            text_wide.pop();
            let mut text_rect = RECT {
                left: icon_right + 7,
                top: 0,
                right: WIDGET_WIDTH,
                bottom: WIDGET_HEIGHT,
            };
            DrawTextW(
                hdc,
                &mut text_wide,
                &mut text_rect,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            SelectObject(hdc, old_font);
            let _ = DeleteObject(hfont);
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_DESTROY => {
            let _ = KillTimer(hwnd, TIMER_UI_REFRESH_ID);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn main() {
    if env::args().any(|a| a == "--diagnose") {
        run_diagnostics();
        return;
    }

    spawn_battery_worker();

    unsafe {
        let instance = GetModuleHandleW(PCWSTR::null()).unwrap_or_default();
        let class_name = to_wstring("RustTaskbarMouseBatteryOverlay");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..zeroed()
        };

        RegisterClassW(&wc);

        let ex_style = WS_EX_LAYERED
            | WS_EX_TRANSPARENT
            | WS_EX_TOPMOST
            | WS_EX_TOOLWINDOW
            | WS_EX_NOACTIVATE;

        let hwnd = CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            WIDGET_WIDTH,
            WIDGET_HEIGHT,
            HWND::default(),
            None,
            instance,
            None,
        )
        .expect("Falha ao criar janela overlay");

        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(TRANSPARENT_KEY), 255, LWA_COLORKEY);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}