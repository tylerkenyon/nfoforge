#![windows_subsystem = "windows"]
#![allow(static_mut_refs)]

use std::{ffi::OsStr, fs, os::windows::ffi::OsStrExt, path::PathBuf, ptr};
use windows_sys::Win32::{
    Foundation::*,
    Globalization::*,
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{Controls::Dialogs::*, Controls::*, Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const EDITOR: usize = 101;
const PREVIEW: usize = 102;
const STATUS: usize = 103;
const GUIDE: usize = 104;
const SUGGESTIONS: usize = 105;
const TEMPLATE: usize = 106;
const NEW: usize = 201;
const OPEN: usize = 202;
const SAVE: usize = 203;
const EXPORT: usize = 204;
const PREVIEW_TOGGLE: usize = 205;
const TIMER: usize = 1;
const PREVIEW_TIMER: usize = 2;
const TAGS: &[&str] = &[
    "[title]",
    "[year]",
    "[genre]",
    "[director]",
    "[producer]",
    "[writer]",
    "[cast]",
    "[plot]",
    "[rating]",
    "[runtime]",
    "[country]",
    "[language]",
    "[source]",
    "[resolution]",
    "[aspect]",
    "[framerate]",
    "[video]",
    "[codec]",
    "[bitrate]",
    "[audio]",
    "[channels]",
    "[subtitles]",
    "[imdb]",
    "[tmdb]",
    "[url]",
    "[site]",
    "[group]",
    "[release]",
    "[files]",
    "[size]",
    "[crc32]",
    "[md5]",
    "[sha1]",
    "[tracker]",
    "[magnet]",
    "[filename]",
    "[file]",
    "[iNFO]",
    "[/iNFO]",
];

struct App {
    editor: HWND,
    preview: HWND,
    status: HWND,
    suggestions: HWND,
    template: HWND,
    visible_preview: bool,
    draft: PathBuf,
    current_path: Option<PathBuf>,
    dirty: bool,
    matches: Vec<String>,
    editor_brush: HBRUSH,
    panel_brush: HBRUSH,
}

static mut APP: Option<App> = None;
static mut OLD_EDITOR_PROC: WNDPROC = None;

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

unsafe fn set_text(hwnd: HWND, value: &str) {
    let text = wide(value);
    unsafe {
        SetWindowTextW(hwnd, text.as_ptr());
    }
}

unsafe fn get_text(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut text = vec![0u16; length + 1];
    let count =
        unsafe { GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32) }.max(0) as usize;
    String::from_utf16_lossy(&text[..count])
}

unsafe fn make_control(
    parent: HWND,
    class: &str,
    text: &str,
    style: WINDOW_STYLE,
    ex: WINDOW_EX_STYLE,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> HWND {
    let class = wide(class);
    let text = wide(text);
    unsafe {
        CreateWindowExW(
            ex,
            class.as_ptr(),
            text.as_ptr(),
            style,
            x,
            y,
            width,
            height,
            parent,
            id as HMENU,
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        )
    }
}

unsafe fn layout(hwnd: HWND) {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe {
        GetClientRect(hwnd, &mut rect);
    }
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let visible = unsafe { APP.as_ref().map(|app| app.visible_preview).unwrap_or(true) };
    let side = (width / 3).clamp(320, 520);
    let editor_width = if visible {
        width - side - 44
    } else {
        width - 32
    };
    let right = editor_width + 28;
    unsafe {
        MoveWindow(
            GetDlgItem(hwnd, EDITOR as i32),
            16,
            88,
            editor_width,
            height - 138,
            1,
        );
        MoveWindow(GetDlgItem(hwnd, GUIDE as i32), right, 88, side, 94, 1);
        MoveWindow(GetDlgItem(hwnd, TEMPLATE as i32), right, 190, side, 250, 1);
        MoveWindow(
            GetDlgItem(hwnd, PREVIEW as i32),
            right,
            250,
            side,
            height - 300,
            1,
        );
        MoveWindow(
            GetDlgItem(hwnd, STATUS as i32),
            16,
            height - 34,
            width - 32,
            22,
            1,
        );
        ShowWindow(
            GetDlgItem(hwnd, GUIDE as i32),
            if visible { SW_SHOW } else { SW_HIDE },
        );
        ShowWindow(
            GetDlgItem(hwnd, TEMPLATE as i32),
            if visible { SW_SHOW } else { SW_HIDE },
        );
        ShowWindow(
            GetDlgItem(hwnd, PREVIEW as i32),
            if visible { SW_SHOW } else { SW_HIDE },
        );
    }
}

fn draft_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("NfoForge")
        .join("draft.nfo")
}

fn decode_file(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&words);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&words);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.trim_start_matches('\u{feff}').to_owned();
    }
    unsafe {
        let count = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            ptr::null_mut(),
            0,
        );
        if count > 0 {
            let mut words = vec![0u16; count as usize];
            let written = MultiByteToWideChar(
                CP_OEMCP,
                0,
                bytes.as_ptr(),
                bytes.len() as i32,
                words.as_mut_ptr(),
                count,
            );
            if written > 0 {
                return String::from_utf16_lossy(&words[..written as usize]);
            }
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_text(path: &PathBuf) -> std::io::Result<String> {
    fs::read(path).map(|bytes| decode_file(&bytes))
}

fn save_text(path: &PathBuf, text: &str) -> std::io::Result<()> {
    fs::write(path, text.as_bytes())
}

fn file_dialog(hwnd: HWND, save: bool) -> Option<PathBuf> {
    let mut file = vec![0u16; 32768];
    let filter =
        wide("NFO files (*.nfo)\0*.nfo\0Text files (*.txt)\0*.txt\0All files (*.*)\0*.*\0\0");
    let title = wide(if save { "Save NFO" } else { "Open NFO" });
    let extension = wide("nfo");
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = hwnd;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.lpstrTitle = title.as_ptr();
    dialog.lpstrDefExt = extension.as_ptr();
    dialog.Flags = OFN_EXPLORER
        | OFN_PATHMUSTEXIST
        | if save {
            OFN_OVERWRITEPROMPT
        } else {
            OFN_FILEMUSTEXIST
        };
    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut dialog)
        } else {
            GetOpenFileNameW(&mut dialog)
        }
    } != 0;
    ok.then(|| {
        PathBuf::from(String::from_utf16_lossy(
            &file[..file.iter().position(|c| *c == 0).unwrap_or(file.len())],
        ))
    })
}

unsafe fn tag_span(editor: HWND) -> Option<(usize, usize, String)> {
    let text = unsafe { get_text(editor) };
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut caret = 0i32;
    let mut end = 0i32;
    unsafe {
        SendMessageW(
            editor,
            EM_GETSEL,
            &mut caret as *mut i32 as usize,
            &mut end as *mut i32 as LPARAM,
        );
    }
    let caret = (caret.max(0) as usize).min(units.len());
    let start = units[..caret]
        .iter()
        .rposition(|unit| *unit == b'[' as u16)?;
    if units[start..caret].iter().any(|unit| {
        *unit == b']' as u16
            || *unit == b'\r' as u16
            || *unit == b'\n' as u16
            || *unit == b' ' as u16
    }) {
        return None;
    }
    let prefix = String::from_utf16_lossy(&units[start..caret]);
    Some((start, caret, prefix))
}

unsafe fn hide_suggestions() {
    unsafe {
        if let Some(app) = APP.as_mut() {
            app.matches.clear();
            ShowWindow(app.suggestions, SW_HIDE);
        }
    }
}

unsafe fn update_suggestions(parent: HWND, force: bool) {
    let editor = unsafe { APP.as_ref().map(|app| app.editor) };
    let Some(editor) = editor else {
        return;
    };
    let prefix = if force {
        Some(String::new())
    } else {
        unsafe { tag_span(editor).map(|(_, _, prefix)| prefix) }
    };
    let Some(prefix) = prefix else {
        unsafe {
            hide_suggestions();
        }
        return;
    };
    let query = prefix.trim_start_matches('[').to_lowercase();
    let matches = TAGS
        .iter()
        .filter(|tag| {
            tag.trim_start_matches('[')
                .to_lowercase()
                .starts_with(&query)
        })
        .map(|tag| tag.to_string())
        .collect::<Vec<_>>();
    if matches.is_empty() {
        unsafe {
            hide_suggestions();
        }
        return;
    }
    unsafe {
        let mut caret = POINT { x: 12, y: 20 };
        GetCaretPos(&mut caret);
        ClientToScreen(editor, &mut caret);
        ScreenToClient(parent, &mut caret);
        let mut rect: RECT = std::mem::zeroed();
        GetClientRect(parent, &mut rect);
        let width = 260.min(rect.right - 32);
        let x = caret.x.clamp(16, rect.right - width - 16);
        let y = (caret.y + 20).clamp(88, rect.bottom - 160);
        if let Some(app) = APP.as_mut() {
            app.matches = matches;
            SendMessageW(app.suggestions, LB_RESETCONTENT, 0, 0);
            for item in &app.matches {
                let item = wide(item);
                SendMessageW(app.suggestions, LB_ADDSTRING, 0, item.as_ptr() as LPARAM);
            }
            SendMessageW(app.suggestions, LB_SETCURSEL, 0, 0);
            let rows = (app.matches.len() as i32).clamp(1, 8);
            SetWindowPos(
                app.suggestions,
                HWND_TOP,
                x,
                y,
                width,
                rows * 24 + 6,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }
}

unsafe fn accept_suggestion() {
    let data = unsafe {
        APP.as_ref().and_then(|app| {
            app.matches
                .first()
                .cloned()
                .map(|first| (app.editor, app.suggestions, first))
        })
    };
    let Some((editor, list, _)) = data else {
        return;
    };
    let index = unsafe { SendMessageW(list, LB_GETCURSEL, 0, 0) };
    if index < 0 {
        return;
    }
    let mut buffer = [0u16; 128];
    unsafe {
        SendMessageW(
            list,
            LB_GETTEXT,
            index as usize,
            buffer.as_mut_ptr() as LPARAM,
        );
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let tag = String::from_utf16_lossy(&buffer[..length]);
    let span = unsafe { tag_span(editor) }.or_else(|| unsafe { caret_span(editor) });
    if let Some((start, end, prefix)) = span {
        let full = unsafe { get_text(editor) };
        let units = full.encode_utf16().collect::<Vec<_>>();
        let end = if !prefix.is_empty() && units.get(end) == Some(&(b']' as u16)) {
            end + 1
        } else {
            end
        };
        unsafe {
            SendMessageW(editor, EM_SETSEL, start, end as LPARAM);
            let replacement = wide(&tag);
            SendMessageW(editor, EM_REPLACESEL, 1, replacement.as_ptr() as LPARAM);
            SetFocus(editor);
            hide_suggestions();
        }
    }
}

unsafe fn caret_span(editor: HWND) -> Option<(usize, usize, String)> {
    let mut caret = 0i32;
    let mut end = 0i32;
    unsafe {
        SendMessageW(
            editor,
            EM_GETSEL,
            &mut caret as *mut i32 as usize,
            &mut end as *mut i32 as LPARAM,
        );
    }
    (caret == end).then_some((caret.max(0) as usize, caret.max(0) as usize, String::new()))
}

unsafe extern "system" fn editor_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_KEYDOWN {
        let visible = unsafe {
            APP.as_ref()
                .map(|app| IsWindowVisible(app.suggestions) != 0)
                .unwrap_or(false)
        };
        if wparam == VK_SPACE as usize && unsafe { GetKeyState(VK_CONTROL as i32) } < 0 {
            unsafe {
                update_suggestions(GetParent(hwnd), true);
            }
            return 0;
        }
        if visible {
            let list = unsafe { APP.as_ref().map(|app| app.suggestions).unwrap() };
            if wparam == VK_DOWN as usize || wparam == VK_UP as usize {
                unsafe {
                    let current = SendMessageW(list, LB_GETCURSEL, 0, 0).max(0);
                    let count = SendMessageW(list, LB_GETCOUNT, 0, 0).max(1);
                    let next = if wparam == VK_DOWN as usize {
                        (current + 1) % count
                    } else {
                        (current + count - 1) % count
                    };
                    SendMessageW(list, LB_SETCURSEL, next as usize, 0);
                }
                return 0;
            }
            if wparam == VK_RETURN as usize || wparam == VK_TAB as usize {
                unsafe {
                    accept_suggestion();
                }
                return 0;
            }
            if wparam == VK_ESCAPE as usize {
                unsafe {
                    hide_suggestions();
                }
                return 0;
            }
        }
    }
    unsafe { CallWindowProcW(OLD_EDITOR_PROC, hwnd, msg, wparam, lparam) }
}

unsafe fn insert_template(editor: HWND, template: &str) {
    let text = wide(template);
    unsafe {
        SendMessageW(editor, EM_REPLACESEL, 1, text.as_ptr() as LPARAM);
        SetFocus(editor);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let toolbar = WS_CHILD | WS_VISIBLE | WS_TABSTOP;
            let editor = unsafe {
                make_control(
                    hwnd,
                    "EDIT",
                    "",
                    toolbar
                        | WS_VSCROLL
                        | WS_HSCROLL
                        | (ES_MULTILINE as u32)
                        | (ES_AUTOVSCROLL as u32)
                        | (ES_AUTOHSCROLL as u32)
                        | (ES_WANTRETURN as u32),
                    WS_EX_CLIENTEDGE,
                    EDITOR,
                    16,
                    88,
                    650,
                    500,
                )
            };
            let preview = unsafe {
                make_control(
                    hwnd,
                    "EDIT",
                    "",
                    WS_CHILD
                        | WS_VISIBLE
                        | WS_VSCROLL
                        | (ES_MULTILINE as u32)
                        | (ES_AUTOVSCROLL as u32)
                        | (ES_READONLY as u32),
                    WS_EX_CLIENTEDGE,
                    PREVIEW,
                    700,
                    250,
                    320,
                    400,
                )
            };
            let _guide = unsafe {
                make_control(hwnd, "STATIC", "TAG COMPLETION\r\nType [ in the editor. Use ↑/↓ then Enter or Tab. Ctrl+Space lists every tag.\r\nNFO is plain text: write any layout or custom markup.", WS_CHILD | WS_VISIBLE, 0, GUIDE, 700, 88, 320, 94)
            };
            let template = unsafe {
                make_control(
                    hwnd,
                    "COMBOBOX",
                    "Choose a starter template...",
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | (CBS_DROPDOWNLIST as u32) | WS_VSCROLL,
                    0,
                    TEMPLATE,
                    700,
                    190,
                    320,
                    250,
                )
            };
            let suggestions = unsafe {
                make_control(
                    hwnd,
                    "LISTBOX",
                    "",
                    WS_CHILD
                        | WS_VSCROLL
                        | WS_BORDER
                        | (LBS_NOTIFY as u32)
                        | (LBS_NOINTEGRALHEIGHT as u32),
                    0,
                    SUGGESTIONS,
                    0,
                    0,
                    260,
                    150,
                )
            };
            let status = unsafe {
                make_control(
                    hwnd,
                    "STATIC",
                    "Ready  ·  UTF-8 export  ·  autosave enabled",
                    WS_CHILD | WS_VISIBLE,
                    0,
                    STATUS,
                    16,
                    680,
                    900,
                    22,
                )
            };
            unsafe {
                make_control(
                    hwnd,
                    "BUTTON",
                    "New",
                    toolbar | (BS_PUSHBUTTON as u32),
                    0,
                    NEW,
                    16,
                    12,
                    72,
                    34,
                );
                make_control(
                    hwnd,
                    "BUTTON",
                    "Open",
                    toolbar | (BS_PUSHBUTTON as u32),
                    0,
                    OPEN,
                    96,
                    12,
                    72,
                    34,
                );
                make_control(
                    hwnd,
                    "BUTTON",
                    "Save",
                    toolbar | (BS_PUSHBUTTON as u32),
                    0,
                    SAVE,
                    176,
                    12,
                    72,
                    34,
                );
                make_control(
                    hwnd,
                    "BUTTON",
                    "Export .nfo",
                    toolbar | (BS_PUSHBUTTON as u32),
                    0,
                    EXPORT,
                    256,
                    12,
                    104,
                    34,
                );
                make_control(
                    hwnd,
                    "BUTTON",
                    "Preview",
                    toolbar | (BS_PUSHBUTTON as u32),
                    0,
                    PREVIEW_TOGGLE,
                    368,
                    12,
                    88,
                    34,
                );
                let name = wide("Consolas");
                let font = CreateFontW(
                    17,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    5,
                    (DEFAULT_PITCH | FF_MODERN) as u32,
                    name.as_ptr(),
                );
                SendMessageW(editor, WM_SETFONT, font as usize, 1);
                SendMessageW(preview, WM_SETFONT, font as usize, 1);
                SendMessageW(editor, 0x00c5, 8 * 1024 * 1024, 0);
                for item in [
                    "Movie / TV",
                    "Music",
                    "Game",
                    "Software",
                    "Scene release",
                    "Blank info block",
                ] {
                    let item = wide(item);
                    SendMessageW(template, CB_ADDSTRING, 0, item.as_ptr() as LPARAM);
                }
                let old =
                    SetWindowLongPtrW(editor, GWLP_WNDPROC, editor_proc as *const () as isize);
                OLD_EDITOR_PROC = Some(std::mem::transmute(old));
                APP = Some(App {
                    editor,
                    preview,
                    status,
                    suggestions,
                    template,
                    visible_preview: true,
                    draft: draft_path(),
                    current_path: None,
                    dirty: false,
                    matches: Vec::new(),
                    editor_brush: CreateSolidBrush(0x00f8f9fb),
                    panel_brush: CreateSolidBrush(0x00eef2f7),
                });
                SetTimer(hwnd, TIMER, 1000, None);
                SetFocus(editor);
                SendMessageW(editor, 0x00c5, 8 * 1024 * 1024, 0);
                if let Some(text) = APP.as_ref().and_then(|app| read_text(&app.draft).ok()) {
                    set_text(editor, &text);
                    set_text(status, "Recovered autosaved draft");
                }
                SendMessageW(editor, 0x00d3, 3, (8 | (8 << 16)) as LPARAM);
                layout(hwnd);
            }
            0
        }
        WM_SIZE => {
            unsafe {
                layout(hwnd);
            }
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT => {
            unsafe {
                let hdc = wparam as HDC;
                let target = lparam as HWND;
                if let Some(app) = APP.as_ref() {
                    if target == app.editor || target == app.preview {
                        SetBkColor(hdc, 0x00f8f9fb);
                        SetTextColor(hdc, 0x00211d18);
                        return app.editor_brush as LRESULT;
                    }
                    SetBkColor(hdc, 0x00eef2f7);
                    SetTextColor(hdc, 0x00211d18);
                    return app.panel_brush as LRESULT;
                }
            }
            0
        }
        WM_TIMER if wparam == PREVIEW_TIMER => {
            unsafe {
                KillTimer(hwnd, PREVIEW_TIMER);
                if let Some(app) = APP.as_ref() {
                    set_text(app.preview, &get_text(app.editor));
                }
            }
            0
        }
        WM_TIMER if wparam == TIMER => {
            unsafe {
                if let Some(app) = APP.as_mut() {
                    let text = get_text(app.editor);
                    if let Some(parent) = app.draft.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if app.dirty && save_text(&app.draft, &text).is_ok() {
                        app.dirty = false;
                        set_text(app.status, "Draft autosaved");
                    }
                }
            }
            0
        }
        WM_COMMAND => {
            let id = (wparam & 0xffff) as usize;
            let notify = ((wparam >> 16) & 0xffff) as u16;
            unsafe {
                if id == EDITOR && notify == EN_CHANGE as u16 {
                    if let Some(app) = APP.as_mut() {
                        app.dirty = true;
                    }
                    KillTimer(hwnd, PREVIEW_TIMER);
                    SetTimer(hwnd, PREVIEW_TIMER, 180, None);
                    update_suggestions(hwnd, false);
                } else if id == TEMPLATE && notify == CBN_SELCHANGE as u16 {
                    let index = APP
                        .as_ref()
                        .map(|app| SendMessageW(app.template, CB_GETCURSEL, 0, 0))
                        .unwrap_or(0);
                    let content = match index {
                        1 => "Artist: \nAlbum: \nYear: \nGenre: \nFormat: \nBitrate: \nTracklist:\n\n01. \n02. \n03. ",
                        2 => "Title: \nPlatform: \nRegion: \nRelease date: \nGenre: \nFormat: \n\nDescription:\n\n",
                        3 => "Title: \nVersion: \nPlatform: \nRelease date: \nLanguage: \n\nDescription:\n\n",
                        4 => "Release: \nGroup: \nDate: \nFiles:\n  01. \nTotal size: \nCRC32: \n",
                        5 => "[iNFO]\n\n[/iNFO]",
                        _ => "Title: \nYear: \nGenre: \nRuntime: \nSource: \nResolution: \nVideo: \nAudio: \nSubtitles: \nIMDB: \n\nPlot:\n\n",
                    };
                    if let Some(app) = APP.as_ref() {
                        insert_template(app.editor, content);
                        SendMessageW(app.template, CB_SETCURSEL, 0, 0);
                    }
                } else if id == SUGGESTIONS && notify == LBN_DBLCLK as u16 {
                    accept_suggestion();
                } else if id == NEW {
                    if let Some(app) = APP.as_mut() {
                        set_text(app.editor, "");
                        app.current_path = None;
                        app.dirty = true;
                        set_text(app.status, "New NFO");
                    }
                } else if id == OPEN {
                    if let Some(path) = file_dialog(hwnd, false) {
                        match read_text(&path) {
                            Ok(text) => {
                                if let Some(app) = APP.as_mut() {
                                    set_text(app.editor, &text);
                                    app.current_path = Some(path);
                                    app.dirty = false;
                                    set_text(app.status, "NFO opened");
                                }
                            }
                            Err(_) => {
                                if let Some(app) = APP.as_ref() {
                                    set_text(app.status, "Could not open that file");
                                }
                            }
                        }
                    }
                } else if id == SAVE || id == EXPORT {
                    let path = if id == EXPORT {
                        file_dialog(hwnd, true)
                    } else {
                        APP.as_ref()
                            .and_then(|app| app.current_path.clone())
                            .or_else(|| file_dialog(hwnd, true))
                    };
                    if let Some(path) = path {
                        let content = APP
                            .as_ref()
                            .map(|app| get_text(app.editor))
                            .unwrap_or_default();
                        match save_text(&path, &content) {
                            Ok(()) => {
                                if let Some(app) = APP.as_mut() {
                                    app.current_path = Some(path);
                                    app.dirty = false;
                                    set_text(app.status, "NFO saved as UTF-8");
                                }
                            }
                            Err(_) => {
                                if let Some(app) = APP.as_ref() {
                                    set_text(app.status, "Could not save that file");
                                }
                            }
                        }
                    }
                } else if id == PREVIEW_TOGGLE {
                    if let Some(app) = APP.as_mut() {
                        app.visible_preview = !app.visible_preview;
                    }
                    layout(hwnd);
                }
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                KillTimer(hwnd, TIMER);
                KillTimer(hwnd, PREVIEW_TIMER);
                if let Some(app) = APP.take() {
                    DeleteObject(app.editor_brush);
                    DeleteObject(app.panel_brush);
                }
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn main() {
    unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let class = wide("NfoForgeWindow");
        let brush = CreateSolidBrush(0x00eef2f7);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
            hbrBackground: brush,
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let title = wide("NfoForge | NFO editor");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1160,
            780,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DeleteObject(brush);
    }
}
