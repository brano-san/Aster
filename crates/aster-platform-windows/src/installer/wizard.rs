use super::{install, uninstall, InstallPaths, Shortcuts};
use aster_core::installation::{self, WizardLabel as Text};
use std::cell::RefCell;
use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_CANCELLED, HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetObjectW, HFONT, HGDIOBJ, LOGFONTW};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    CheckDlgButton, InitCommonControlsEx, IsDlgButtonChecked, BST_CHECKED, BST_UNCHECKED, EM_LIMITTEXT,
    ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETMARQUEE,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FORCEFILESYSTEM,
    FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const TEMPLATE: PCWSTR = PCWSTR(101 as _);
const HEADING: i32 = 1001;
const DESCRIPTION: i32 = 1002;
const FOLDER_LABEL: i32 = 1003;
const FOLDER: i32 = 1004;
const BROWSE: i32 = 1005;
const START_MENU: i32 = 1006;
const DESKTOP: i32 = 1007;
const EXTRA_OPTION: i32 = 1008;
const PROGRESS: i32 = 1009;
const DETAILS: i32 = 1010;
const BACK: i32 = 1011;
const LOGO: i32 = 1012;
const NEXT: i32 = IDOK.0;
const CANCEL: i32 = IDCANCEL.0;
const WORK_TIMER: usize = 1;

enum Page {
    Welcome,
    Options,
    Working(JoinHandle<io::Result<()>>),
    Complete,
    Error(String),
}

type CloseSessions = Arc<dyn Fn() -> io::Result<()> + Send + Sync>;

struct Wizard {
    installing: bool,
    existing: bool,
    paths: InstallPaths,
    source: PathBuf,
    version: String,
    close_sessions: CloseSessions,
    page: Page,
    heading_font: HFONT,
    logo: HICON,
}

pub(super) fn run(
    installing: bool,
    paths: InstallPaths,
    source: &Path,
    version: &str,
    close_sessions: impl Fn() -> io::Result<()> + Send + Sync + 'static,
) -> io::Result<i32> {
    let existing = paths.exe.is_file();
    if !installing && !existing {
        return Err(io::Error::new(io::ErrorKind::NotFound, installation::not_installed()));
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let controls = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_PROGRESS_CLASS };
        if !InitCommonControlsEx(&controls).as_bool() {
            return Err(io::Error::last_os_error());
        }
    }
    let state = RefCell::new(Wizard {
        installing,
        existing,
        paths,
        source: source.to_owned(),
        version: version.to_owned(),
        close_sessions: Arc::new(close_sessions),
        page: if installing { Page::Welcome } else { Page::Options },
        heading_font: HFONT::default(),
        logo: HICON::default(),
    });
    let result = unsafe { DialogBoxParamW(None, TEMPLATE, None, Some(dialog), LPARAM(&state as *const _ as isize)) };
    if result == -1 { Err(io::Error::last_os_error()) } else { Ok(result as i32) }
}

impl Wizard {
    fn init(&mut self, window: HWND) {
        let title = if self.installing { Text::Setup } else { Text::Uninstall };
        let (start, desktop) = if self.existing { crate::shortcut::installed_shortcuts(&self.paths.exe) } else { (true, false) };
        unsafe {
            let _ = SetWindowTextW(window, &HSTRING::from(title.text()));
            if let Ok(module) = GetModuleHandleW(None) {
                let module = Some(HINSTANCE(module.0));
                if let Ok(icon) = LoadIconW(module, crate::APP_ICON) {
                    SendMessageW(window, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(icon.0 as isize)));
                }
                let size = (40 * GetDpiForWindow(window).max(96) / 96) as i32;
                if let Ok(icon) = LoadImageW(module, crate::APP_ICON, IMAGE_ICON, size, size, IMAGE_FLAGS(0)) {
                    self.logo = HICON(icon.0);
                    send(window, LOGO, STM_SETICON, icon.0 as usize, 0);
                }
            }
            let mut font = LOGFONTW::default();
            let base = SendMessageW(window, WM_GETFONT, None, None);
            if GetObjectW(HGDIOBJ(base.0 as _), size_of::<LOGFONTW>() as i32, Some((&mut font as *mut LOGFONTW).cast())) != 0 {
                font.lfHeight = font.lfHeight * 5 / 3;
                font.lfWeight = 600;
                self.heading_font = CreateFontIndirectW(&font);
                send(window, HEADING, WM_SETFONT, self.heading_font.0 as usize, 1);
            }
        }
        send(window, FOLDER, EM_LIMITTEXT, 32766, 0);
        text(window, FOLDER, self.paths.dir.as_os_str());
        for (id, label) in [
            (FOLDER_LABEL, Text::Folder), (BROWSE, Text::Browse), (START_MENU, Text::StartMenu),
            (DESKTOP, Text::Desktop), (BACK, Text::Back), (CANCEL, Text::Cancel),
        ] {
            text(window, id, label.text());
        }
        check(window, START_MENU, start);
        check(window, DESKTOP, desktop);
        self.render(window);
    }

    fn render(&self, window: HWND) {
        let options = matches!(self.page, Page::Options);
        let working = matches!(self.page, Page::Working(_));
        let complete = matches!(self.page, Page::Complete);
        let setup_options = options && self.installing;
        for id in [FOLDER_LABEL, FOLDER, START_MENU, DESKTOP] {
            show(window, id, setup_options);
        }
        show(window, BROWSE, setup_options && !self.existing);
        enable(window, FOLDER, !self.existing);
        show(window, BACK, setup_options);
        show(window, EXTRA_OPTION, (options && !self.installing) || (complete && self.installing));
        show(window, PROGRESS, working);
        show(window, DETAILS, matches!(self.page, Page::Error(_)));
        enable(window, NEXT, !working);
        show(window, CANCEL, !complete);
        enable(window, CANCEL, !working);
        send(window, PROGRESS, PBM_SETMARQUEE, usize::from(working), 30);

        let (heading, description, primary) = match &self.page {
            Page::Welcome => (Text::Welcome, installation::welcome(&self.version, self.existing), Text::Next),
            Page::Options if self.installing => {
                let hint = if self.existing { Text::UpgradeOptions } else { Text::ChooseOptions };
                (Text::Options, hint.text().to_owned(), Text::Install)
            }
            Page::Options => {
                text(window, EXTRA_OPTION, installation::delete_settings());
                (Text::Uninstall, Text::KeepData.text().to_owned(), Text::Remove)
            }
            Page::Working(_) if self.installing => (Text::Installing, Text::ReplacingFiles.text().to_owned(), Text::Install),
            Page::Working(_) => (Text::Removing, Text::RemovingIntegration.text().to_owned(), Text::Remove),
            Page::Complete if self.installing => {
                text(window, EXTRA_OPTION, Text::Launch.text());
                check(window, EXTRA_OPTION, true);
                (Text::Installed, installation::result(true).to_owned(), Text::Finish)
            }
            Page::Complete => (Text::Uninstalled, installation::result(false).to_owned(), Text::Finish),
            Page::Error(error) => {
                text(window, DETAILS, error.replace('\n', "\r\n"));
                (Text::Error, String::new(), Text::Retry)
            }
        };
        text(window, HEADING, heading.text());
        text(window, DESCRIPTION, description);
        text(window, NEXT, primary.text());
        if !working {
            if let Ok(button) = unsafe { GetDlgItem(Some(window), NEXT) } {
                unsafe { let _ = PostMessageW(Some(window), WM_NEXTDLGCTL, WPARAM(button.0 as usize), LPARAM(1)); }
            }
        }
    }

    fn start(&mut self, window: HWND) -> io::Result<()> {
        let paths = if self.installing && !self.existing {
            InstallPaths::from_dir(PathBuf::from(input(window, FOLDER)))?
        } else {
            self.paths.clone()
        };
        let shortcuts = Shortcuts { start_menu: checked(window, START_MENU), desktop: checked(window, DESKTOP) };
        let delete_settings = !self.installing && checked(window, EXTRA_OPTION);
        let (installing, source, version, close_sessions) = (self.installing, self.source.clone(), self.version.clone(), self.close_sessions.clone());
        if unsafe { SetTimer(Some(window), WORK_TIMER, 60, None) } == 0 {
            return Err(io::Error::last_os_error());
        }
        self.paths = paths.clone();
        let worker = thread::Builder::new().name("aster-setup".into()).spawn(move || {
            if installing {
                install(&paths, &source, &version, shortcuts, true)
            } else {
                uninstall(&paths, &source, delete_settings, || close_sessions())
            }
        });
        match worker {
            Ok(worker) => { self.page = Page::Working(worker); }
            Err(error) => {
                unsafe { let _ = KillTimer(Some(window), WORK_TIMER); }
                return Err(error);
            }
        }
        Ok(())
    }

    fn next(&mut self, window: HWND) {
        match self.page {
            Page::Welcome | Page::Error(_) => self.page = Page::Options,
            Page::Options => {
                if let Err(error) = self.start(window) {
                    self.page = Page::Error(installation::failed(self.installing, &error.to_string()));
                }
            }
            Page::Complete => {
                if self.installing && checked(window, EXTRA_OPTION) {
                    if let Err(error) = Command::new(&self.paths.exe).current_dir(&self.paths.dir).spawn() {
                        text(window, DESCRIPTION, error.to_string());
                        check(window, EXTRA_OPTION, false);
                        return;
                    }
                }
                unsafe { let _ = EndDialog(window, 0); }
                return;
            }
            Page::Working(_) => return,
        }
        self.render(window);
    }

    fn poll(&mut self, window: HWND) {
        if !matches!(&self.page, Page::Working(worker) if worker.is_finished()) {
            return;
        }
        unsafe { let _ = KillTimer(Some(window), WORK_TIMER); }
        if let Page::Working(worker) = std::mem::replace(&mut self.page, Page::Complete) {
            self.page = match worker.join() {
                Ok(Ok(())) => Page::Complete,
                Ok(Err(error)) => Page::Error(installation::failed(self.installing, &error.to_string())),
                Err(_) => Page::Error(installation::failed(self.installing, Text::Error.text())),
            };
        }
        self.render(window);
    }

    fn close(&self, window: HWND) {
        if !matches!(self.page, Page::Working(_)) {
            let code = isize::from(matches!(self.page, Page::Error(_)));
            unsafe { let _ = EndDialog(window, code); }
        }
    }
}

unsafe extern "system" fn dialog(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    if message == WM_INITDIALOG {
        SetWindowLongPtrW(window, GWLP_USERDATA, lparam.0);
    }
    let state = GetWindowLongPtrW(window, GWLP_USERDATA) as *const RefCell<Wizard>;
    let Some(state) = state.as_ref() else { return 0 };
    let Ok(mut state) = state.try_borrow_mut() else { return 0 };
    match message {
        WM_INITDIALOG => state.init(window),
        WM_CLOSE => state.close(window),
        WM_COMMAND => match (wparam.0 & 0xffff) as i32 {
            NEXT => state.next(window),
            CANCEL => state.close(window),
            BACK if state.installing && matches!(state.page, Page::Options) => {
                state.page = Page::Welcome;
                state.render(window);
            }
            BROWSE if state.installing && !state.existing && matches!(state.page, Page::Options) && wparam.0 >> 16 == 0 => {
                let current = PathBuf::from(input(window, FOLDER));
                match browse(window, &current) {
                    Ok(Some(path)) => text(window, FOLDER, path.as_os_str()),
                    Ok(None) => {}
                    Err(error) => text(window, DESCRIPTION, error.to_string()),
                }
            }
            _ => return 0,
        },
        WM_TIMER if wparam.0 == WORK_TIMER => state.poll(window),
        _ => return 0,
    }
    1
}

impl Drop for Wizard {
    fn drop(&mut self) {
        if !self.heading_font.is_invalid() {
            unsafe { let _ = DeleteObject(HGDIOBJ(self.heading_font.0)); }
        }
        if !self.logo.is_invalid() {
            unsafe { let _ = DestroyIcon(self.logo); }
        }
    }
}

fn text(window: HWND, id: i32, value: impl AsRef<std::ffi::OsStr>) {
    unsafe { let _ = SetDlgItemTextW(window, id, &HSTRING::from(value.as_ref())); }
}

fn input(window: HWND, id: i32) -> OsString {
    let mut buffer = vec![0u16; 32768];
    let len = unsafe { GetDlgItemTextW(window, id, &mut buffer) } as usize;
    OsString::from_wide(&buffer[..len])
}

fn show(window: HWND, id: i32, visible: bool) {
    if let Ok(control) = unsafe { GetDlgItem(Some(window), id) } {
        unsafe { let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE }); }
    }
}

fn enable(window: HWND, id: i32, enabled: bool) {
    if let Ok(control) = unsafe { GetDlgItem(Some(window), id) } {
        unsafe { let _ = EnableWindow(control, enabled); }
    }
}

fn check(window: HWND, id: i32, value: bool) {
    unsafe { let _ = CheckDlgButton(window, id, if value { BST_CHECKED } else { BST_UNCHECKED }); }
}

fn checked(window: HWND, id: i32) -> bool {
    (unsafe { IsDlgButtonChecked(window, id) }) == BST_CHECKED.0
}

fn send(window: HWND, id: i32, message: u32, wparam: usize, lparam: isize) {
    if let Ok(control) = unsafe { GetDlgItem(Some(window), id) } {
        unsafe { SendMessageW(control, message, Some(WPARAM(wparam)), Some(LPARAM(lparam))); }
    }
}

fn browse(window: HWND, current: &Path) -> windows::core::Result<Option<PathBuf>> {
    let _apartment = crate::shell_link::Apartment::enter()?;
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)?;
        dialog.SetTitle(&HSTRING::from(Text::Folder.text()))?;
        if let Some(parent) = current.ancestors().find(|path| path.is_dir()) {
            if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(parent), None) {
                dialog.SetFolder(&folder)?;
            }
        }
        match dialog.Show(Some(window)) {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_CANCELLED.to_hresult() => return Ok(None),
            Err(error) => return Err(error),
        }
        let value = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
        let folder = OsString::from_wide(value.as_wide());
        CoTaskMemFree(Some(value.0.cast()));
        Ok(Some(PathBuf::from(folder)))
    }
}
