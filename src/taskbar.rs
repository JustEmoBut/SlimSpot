//! Taskbar button extras through ITaskbarList3: previous / play-pause / next under the window
//! preview (thumbnail toolbar) and the playing track's progress on the button itself.

use std::cell::Cell;
use std::time::Duration;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use slint::ComponentHandle;
use tokio::sync::mpsc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateBitmap;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Shell::{
    DefSubclassProc, ITaskbarList3, SetWindowSubclass, TBPF_NOPROGRESS, TBPF_NORMAL, TBPF_PAUSED, TaskbarList, THB_FLAGS, THB_ICON,
    THB_TOOLTIP, THBF_ENABLED, THBN_CLICKED, THUMBBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO, RegisterWindowMessageW, WM_COMMAND};
use windows::core::w;

use crate::player::Command;
use crate::ui::App;

// Small-icon size the thumbnail toolbar draws at 100% scaling (SM_CXSMICON).
const ICON_SIZE: usize = 16;
// Same cadence as the seek bar ticker in main.rs.
const PROGRESS_TICK: Duration = Duration::from_millis(500);
// Arbitrary id for our window subclass.
const SUBCLASS_ID: usize = 1;
const PREV: u32 = 0;
const TOGGLE: u32 = 1;
const NEXT: u32 = 2;

/// Lives for the rest of the program (leaked): the window subclass and the timer use it.
struct Taskbar {
    list: ITaskbarList3,
    hwnd: HWND,
    tx: mpsc::UnboundedSender<Command>,
    /// prev, play, pause, next
    icons: [HICON; 4],
    /// Explorer sends this when it (re)creates our taskbar button, e.g. after the window was hidden.
    button_created: u32,
    playing: Cell<bool>,
    timer: slint::Timer,
}

impl Taskbar {
    fn buttons(&self) -> [THUMBBUTTON; 3] {
        let playing = self.playing.get();
        let button = |id: u32, icon: HICON, tip: &str| {
            let mut b = THUMBBUTTON { dwMask: THB_ICON | THB_TOOLTIP | THB_FLAGS, iId: id, hIcon: icon, dwFlags: THBF_ENABLED, ..Default::default() };
            for (dst, src) in b.szTip.iter_mut().zip(tip.encode_utf16()) {
                *dst = src;
            }
            b
        };
        [
            button(PREV, self.icons[0], "Previous"),
            if playing { button(TOGGLE, self.icons[2], "Pause") } else { button(TOGGLE, self.icons[1], "Play") },
            button(NEXT, self.icons[3], "Next"),
        ]
    }

    fn add_buttons(&self) {
        // Fails when the button doesn't exist yet; `button_created` arrives later and retries.
        let _ = unsafe { self.list.ThumbBarAddButtons(self.hwnd, &self.buttons()) };
    }

    fn update(&self, app: &App) {
        let playing = app.get_playing();
        if playing != self.playing.replace(playing) {
            let _ = unsafe { self.list.ThumbBarUpdateButtons(self.hwnd, &self.buttons()) };
        }
        let (position, duration) = (app.get_position() as u64, app.get_duration() as u64);
        unsafe {
            if duration == 0 {
                let _ = self.list.SetProgressState(self.hwnd, TBPF_NOPROGRESS);
            } else {
                let _ = self.list.SetProgressState(self.hwnd, if playing { TBPF_NORMAL } else { TBPF_PAUSED });
                let _ = self.list.SetProgressValue(self.hwnd, position.min(duration), duration);
            }
        }
    }
}

unsafe extern "system" fn subclass_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, data: usize) -> LRESULT {
    let taskbar = unsafe { &*(data as *const Taskbar) };
    if msg == taskbar.button_created {
        taskbar.add_buttons();
    } else if msg == WM_COMMAND && (wparam.0 >> 16) as u32 == THBN_CLICKED {
        let cmd = match (wparam.0 & 0xffff) as u32 {
            PREV => Command::Prev,
            TOGGLE => Command::Toggle,
            NEXT => Command::Next,
            _ => return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
        };
        let _ = taskbar.tx.send(cmd);
        return LRESULT(0);
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Call once the native window exists (same moment as the media keys). Best-effort: the
/// player works without it, so failures are only logged.
pub fn setup(app: &App, tx: &mpsc::UnboundedSender<Command>) {
    let Ok(RawWindowHandle::Win32(h)) = app.window().window_handle().window_handle().map(|h| h.as_raw()) else { return };
    let hwnd = HWND(h.hwnd.get() as _);
    let list: ITaskbarList3 = match unsafe {
        // Already initialised as STA on the UI thread by winit; this only makes sure.
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER)
    } {
        Ok(list) => list,
        Err(e) => return log::warn!("taskbar buttons unavailable: {e}"),
    };
    if let Err(e) = unsafe { list.HrInit() } {
        return log::warn!("taskbar buttons unavailable: {e}");
    }
    let icons = [glyph(prev_shape), glyph(play_shape), glyph(pause_shape), glyph(next_shape)];
    let taskbar: &'static Taskbar = Box::leak(Box::new(Taskbar {
        list,
        hwnd,
        tx: tx.clone(),
        icons,
        button_created: unsafe { RegisterWindowMessageW(w!("TaskbarButtonCreated")) },
        playing: Cell::new(false),
        timer: slint::Timer::default(),
    }));
    if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, taskbar as *const Taskbar as usize) }.as_bool() {
        return log::warn!("taskbar buttons unavailable: SetWindowSubclass failed");
    }
    taskbar.add_buttons();
    let weak = app.as_weak();
    taskbar.timer.start(slint::TimerMode::Repeated, PROGRESS_TICK, move || {
        if let Some(app) = weak.upgrade() {
            taskbar.update(&app);
        }
    });
}

/// A white 16 px icon from a shape test in 0..16 coordinates, 4×4 supersampled for soft edges.
fn glyph(inside: fn(f32, f32) -> bool) -> HICON {
    const SAMPLES: usize = 4;
    let mut bgra = vec![0u8; ICON_SIZE * ICON_SIZE * 4];
    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let mut hits = 0;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let (fx, fy) = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32, y as f32 + (sy as f32 + 0.5) / SAMPLES as f32);
                    hits += inside(fx, fy) as usize;
                }
            }
            let alpha = (hits * 255 / (SAMPLES * SAMPLES)) as u8;
            bgra[(y * ICON_SIZE + x) * 4..][..4].copy_from_slice(&[255, 255, 255, alpha]);
        }
    }
    let size = ICON_SIZE as i32;
    let mask = vec![0u8; ICON_SIZE * ICON_SIZE / 8];
    unsafe {
        let info = ICONINFO {
            fIcon: true.into(),
            hbmColor: CreateBitmap(size, size, 1, 32, Some(bgra.as_ptr().cast())),
            hbmMask: CreateBitmap(size, size, 1, 1, Some(mask.as_ptr().cast())),
            ..Default::default()
        };
        // ponytail: the bitmaps and icons are never freed; four 1 KB icons for the program's lifetime.
        CreateIconIndirect(&info).unwrap_or_default()
    }
}

/// Right-pointing triangle from x = `left` to its tip at `right`, centered on y = 8.
fn triangle(x: f32, y: f32, left: f32, right: f32, half_height: f32) -> bool {
    x >= left && x <= right && (y - 8.0).abs() <= half_height * (right - x) / (right - left)
}

fn play_shape(x: f32, y: f32) -> bool {
    triangle(x, y, 4.0, 13.5, 6.0)
}

fn pause_shape(x: f32, y: f32) -> bool {
    (2.0..14.0).contains(&y) && ((3.5..6.5).contains(&x) || (9.5..12.5).contains(&x))
}

fn next_shape(x: f32, y: f32) -> bool {
    triangle(x, y, 2.5, 10.5, 5.5) || ((10.5..13.0).contains(&x) && (2.5..13.5).contains(&y))
}

fn prev_shape(x: f32, y: f32) -> bool {
    next_shape(16.0 - x, y)
}
