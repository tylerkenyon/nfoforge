#![windows_subsystem = "windows"]
#![allow(static_mut_refs)]

use std::{ffi::OsStr, fs, os::windows::ffi::OsStrExt, path::PathBuf, ptr};
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleW, UI::{Controls::*, Controls::Dialogs::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*}};

const EDITOR: usize = 101;
const PREVIEW: usize = 102;
const TAGS: usize = 103;
const STATUS: usize = 104;
const GUIDE: usize = 105;
const NEW: usize = 201;
const OPEN: usize = 202;
const EXPORT: usize = 203;
const TOGGLE: usize = 204;
const TIMER: usize = 1;

struct App {
    editor: HWND,
    preview: HWND,
    tags: HWND,
    status: HWND,
    visible_preview: bool,
    draft: PathBuf,
}

static mut APP: Option<App> = None;

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

unsafe fn set_text(hwnd: HWND, value: &str) {
    let text = wide(value);
    unsafe { SetWindowTextW(hwnd, text.as_ptr()); }
}

unsafe fn get_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut text = vec![0u16; length + 1];
    let count = unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) }.max(0) as usize;
    String::from_utf16_lossy(&text[..count])
}

unsafe fn make_control(parent: HWND, class: &str, text: &str, style: WINDOW_STYLE, ex: WINDOW_EX_STYLE, id: usize, x: i32, y: i32, width: i32, height: i32) -> HWND {
    let class = wide(class);
    let text = wide(text);
    unsafe { CreateWindowExW(ex, class.as_ptr(), text.as_ptr(), style, x, y, width, height, parent, id as HMENU, GetModuleHandleW(ptr::null()), ptr::null()) }
}

unsafe fn layout(hwnd: HWND) {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut rect); }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let show_preview = unsafe { APP.as_ref().map(|app| app.visible_preview).unwrap_or(true) };
    let edit_width = if show_preview { (width - 328).max(220) } else { width - 24 };
    let right = (width - 304).max(236);
    unsafe {
        MoveWindow(GetDlgItem(hwnd, EDITOR as i32), 12, 54, edit_width, height - 98, 1);
        MoveWindow(GetDlgItem(hwnd, PREVIEW as i32), right, 190, 292, height - 234, 1);
        MoveWindow(GetDlgItem(hwnd, TAGS as i32), right, 54, 292, 280, 1);
        MoveWindow(GetDlgItem(hwnd, GUIDE as i32), right, 94, 292, 88, 1);
        MoveWindow(GetDlgItem(hwnd, STATUS as i32), 12, height - 34, width - 24, 22, 1);
    }
}

fn draft_path() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("NfoForge").join("draft.nfo")
}

fn file_dialog(hwnd: HWND, save: bool) -> Option<PathBuf> {
    let mut file = vec![0u16; 32768];
    let filter = wide("NFO files (*.nfo)\0*.nfo\0Text files (*.txt)\0*.txt\0All files (*.*)\0*.*\0\0");
    let title = wide(if save { "Export NFO" } else { "Open NFO" });
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    let extension = wide("nfo");
    dialog.lpstrDefExt = extension.as_ptr();
    dialog.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST };
    let ok = unsafe { if save { GetSaveFileNameW(&mut dialog) } else { GetOpenFileNameW(&mut dialog) } } != 0;
    ok.then(|| PathBuf::from(String::from_utf16_lossy(&file[..file.iter().position(|c| *c == 0).unwrap_or(file.len())])))
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let editor = unsafe { make_control(hwnd, "EDIT", "", WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL | (ES_MULTILINE as u32) | (ES_AUTOVSCROLL as u32) | (ES_AUTOHSCROLL as u32) | (ES_WANTRETURN as u32) | WS_TABSTOP, WS_EX_CLIENTEDGE, EDITOR, 12, 54, 600, 500) };
            let preview = unsafe { make_control(hwnd, "EDIT", "", WS_CHILD | WS_VISIBLE | WS_VSCROLL | (ES_MULTILINE as u32) | (ES_AUTOVSCROLL as u32) | (ES_READONLY as u32), WS_EX_CLIENTEDGE, PREVIEW, 620, 94, 292, 460) };
            let tags = unsafe { make_control(hwnd, "COMBOBOX", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | (CBS_DROPDOWNLIST as u32) | WS_VSCROLL, 0, TAGS, 620, 54, 292, 280) };
            let status = unsafe { make_control(hwnd, "STATIC", "Ready", WS_CHILD | WS_VISIBLE, 0, STATUS, 12, 650, 800, 22) };
            let _guide = unsafe { make_control(hwnd, "STATIC", "TAG GUIDE    Pick a tag above to insert it.\r\nUse [iNFO] and [/iNFO] for a block.\r\nThe preview mirrors your text.", WS_CHILD | WS_VISIBLE, 0, GUIDE, 620, 94, 292, 88) };
            unsafe {
                make_control(hwnd, "BUTTON", "New", WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32), 0, NEW, 12, 10, 68, 30);
                make_control(hwnd, "BUTTON", "Open", WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32), 0, OPEN, 88, 10, 68, 30);
                make_control(hwnd, "BUTTON", "Export .nfo", WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32), 0, EXPORT, 164, 10, 100, 30);
                make_control(hwnd, "BUTTON", "Toggle preview", WS_CHILD | WS_VISIBLE | WS_TABSTOP | (BS_PUSHBUTTON as u32), 0, TOGGLE, 272, 10, 120, 30);
                let name = wide("Consolas");
                let font = CreateFontW(17, 0, 0, 0, FW_NORMAL as i32, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, 5, (DEFAULT_PITCH | FF_MODERN) as u32, name.as_ptr());
                SendMessageW(editor, WM_SETFONT, font as usize, 1);
                SendMessageW(preview, WM_SETFONT, font as usize, 1);
                for item in ["[title]", "[size]", "[link]", "[url]", "[tracker]", "[file]", "[filename]", "[year]", "[genre]", "[rip]", "[codec]", "[resolution]", "[audio]", "[video]", "[imdb]", "[iNFO]", "[/iNFO]"] {
                    let item = wide(item);
                    SendMessageW(tags, CB_ADDSTRING, 0, item.as_ptr() as LPARAM);
                }
                APP = Some(App { editor, preview, tags, status, visible_preview: true, draft: draft_path() });
                SetTimer(hwnd, TIMER, 1200, None);
                let restore = APP.as_ref().and_then(|a| fs::read_to_string(&a.draft).ok());
                if let Some(text) = restore { set_text(editor, &text); set_text(status, "Recovered autosaved draft"); }
                layout(hwnd);
            }
            0
        }
        WM_SIZE => { unsafe { layout(hwnd); } 0 }
        WM_TIMER if wparam == TIMER => {
            unsafe {
                if let Some(app) = APP.as_ref() {
                    let text = get_text(app.editor);
                    if let Some(parent) = app.draft.parent() { let _ = fs::create_dir_all(parent); }
                    if fs::write(&app.draft, text.as_bytes()).is_ok() { set_text(app.status, "Autosaved locally"); }
                }
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as usize;
            let notify = ((wparam >> 16) & 0xffff) as u16;
            unsafe {
                if id == EDITOR && notify == EN_CHANGE as u16 {
                    if let Some(app) = APP.as_ref() { set_text(app.preview, &get_text(app.editor)); }
                } else if id == TAGS && notify == CBN_SELCHANGE as u16 {
                    if let Some(app) = APP.as_ref() {
                        let index = SendMessageW(app.tags, CB_GETCURSEL, 0, 0) as usize;
                        let mut buf = [0u16; 64];
                        SendMessageW(app.tags, CB_GETLBTEXT, index, buf.as_mut_ptr() as LPARAM);
                        let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
                        let snippet = String::from_utf16_lossy(&buf[..len]);
                        let insertion = wide(&format!("{snippet}\\r\\n"));
                        SendMessageW(app.editor, EM_REPLACESEL, 1, insertion.as_ptr() as LPARAM);
                        SetFocus(app.editor);
                    }
                } else if id == NEW {
                    if let Some(app) = APP.as_mut() { set_text(app.editor, ""); set_text(app.status, "New document"); }
                } else if id == OPEN {
                    if let Some(path) = file_dialog(hwnd, false) {
                        if let Ok(text) = fs::read_to_string(path) {
                            if let Some(app) = APP.as_mut() { set_text(app.editor, &text); set_text(app.status, "Opened NFO file"); }
                        }
                    }
                } else if id == EXPORT {
                    if let Some(path) = file_dialog(hwnd, true) {
                        if let Some(app) = APP.as_ref() {
                            if fs::write(&path, get_text(app.editor).as_bytes()).is_ok() { set_text(app.status, &format!("Exported {}", path.display())); }
                        }
                    }
                } else if id == TOGGLE {
                    if let Some(app) = APP.as_mut() {
                        app.visible_preview = !app.visible_preview;
                        ShowWindow(app.preview, if app.visible_preview { SW_SHOW } else { SW_HIDE });
                        ShowWindow(app.tags, if app.visible_preview { SW_SHOW } else { SW_HIDE });
                        ShowWindow(GetDlgItem(hwnd, GUIDE as i32), if app.visible_preview { SW_SHOW } else { SW_HIDE });
                    }
                }
            }
            0
        }
        WM_DESTROY => { unsafe { KillTimer(hwnd, TIMER); PostQuitMessage(0); } 0 }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }
}

fn main() {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = wide("NfoForgeWindow");
        let wc = WNDCLASSW { lpfnWndProc: Some(window_proc), hInstance: instance, lpszClassName: class.as_ptr(), hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW), hbrBackground: (GetStockObject(WHITE_BRUSH) as HBRUSH), ..std::mem::zeroed() };
        RegisterClassW(&wc);
        let title = wide("NfoForge — NFO Editor");
        let hwnd = CreateWindowExW(0, class.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, 1040, 720, ptr::null_mut(), ptr::null_mut(), instance, ptr::null());
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 { TranslateMessage(&msg); DispatchMessageW(&msg); }
    }
}






