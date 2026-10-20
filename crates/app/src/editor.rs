//! Plugin editors: each plugin's own interface, in a native window of its own that
//! floats over Pulse (and hides while Pulse is in the background), as in other DAWs.
//! The window follows the sizes the editor asks for, and the editor follows the
//! window when the user resizes it.
//!
//! macOS: an NSWindow at the floating level that hides with Pulse. Windows: a
//! window owned by Pulse's (it stays above it and minimizes with it), in pixels,
//! with the editor told the screen's scale; it follows the window to a screen
//! of another scale.

use crate::app::PulseApp;
use engine::model::PluginId;
use std::collections::HashMap;

/// The open editors, by plugin.
#[derive(Default)]
pub struct Editors {
    #[cfg(target_os = "macos")]
    open: HashMap<PluginId, mac::Editor>,
    #[cfg(windows)]
    open: HashMap<PluginId, win::Editor>,
    #[cfg(not(any(target_os = "macos", windows)))]
    open: HashMap<PluginId, ()>,
    /// Editors to open again once their plugins are reloaded (after undo or opening).
    reopen: Vec<PluginId>,
}

impl Editors {
    pub fn is_open(&self, id: PluginId) -> bool {
        self.open.contains_key(&id)
    }
}

impl PulseApp {
    /// Opens plugin `id`'s editor, or brings it to the front if it is open.
    pub fn open_editor(&mut self, id: PluginId) {
        let name = self.plugin_name(id);
        let Some(handle) = self.samples.plugins.get(&id) else {
            self.status = Some(format!("{name} no está cargado"));
            return;
        };
        #[cfg(any(target_os = "macos", windows))]
        {
            if let Some(editor) = self.editors.open.get(&id) {
                editor.front();
                return;
            }
            #[cfg(target_os = "macos")]
            let opened = mac::Editor::open(handle, &name);
            #[cfg(windows)]
            let opened = win::Editor::open(handle, &name);
            match opened {
                Ok(editor) => {
                    self.editors.open.insert(id, editor);
                }
                Err(error) => {
                    eprintln!("[editor] {name}: {error}");
                    self.status = Some(format!("{name}: {error}"));
                }
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = handle;
            self.status = Some(format!("{name}: en este sistema los editores de plugins aún no se abren; usa Parámetros…"));
        }
    }

    /// Closes every editor before their plugins are reloaded, to open them again after.
    pub fn suspend_editors(&mut self) {
        let ids: Vec<PluginId> = self.editors.open.keys().copied().collect();
        self.editors.reopen.extend(ids);
        self.editors.open.clear();
    }

    /// Once a frame: drops editors whose window was closed or whose plugin went away,
    /// follows window resizes, and reopens suspended editors.
    pub fn poll_editors(&mut self) {
        // CLAP plugins may have asked to run something on this thread.
        for handle in self.samples.plugins.values() {
            handle.idle();
        }
        let plugins = &self.samples.plugins;
        self.editors.open.retain(|id, _editor| {
            #[cfg(any(target_os = "macos", windows))]
            if !_editor.poll() {
                return false;
            }
            plugins.contains_key(id)
        });
        for id in std::mem::take(&mut self.editors.reopen) {
            if self.samples.plugins.contains_key(&id) {
                self.open_editor(id);
            }
        }
    }
}

/// Checks the editor of every installed plugin without showing anything: each one is
/// put into a native view that is in no window, resized, and taken out again. Must run
/// on the main thread (from `main`).
pub fn check_installed() -> Vec<String> {
    #[cfg(target_os = "macos")]
    let mut report = vec![format!("Ventanas de editor: {}", mac::check_closing())];
    #[cfg(windows)]
    let mut report = vec![format!("Ventanas de editor: {}", win::check_closing())];
    #[cfg(not(any(target_os = "macos", windows)))]
    let mut report = Vec::new();
    for bundle in engine::plugin::default_folders().iter().flat_map(|f| engine::plugin::bundles(f)) {
        let Ok(list) = engine::plugin::scan(&bundle) else { continue };
        for info in list {
            let line = match engine::plugin::instantiate(&info.path, &info.cid, 48_000.0, None) {
                Ok((_processor, handle)) => check_editor(&handle),
                Err(error) => error,
            };
            let format = if engine::plugin::is_clap(&info.path) { "CLAP" } else { "VST3" };
            report.push(format!("{} [{format}]: {line}", info.name));
        }
    }
    report
}

#[cfg(target_os = "macos")]
fn check_editor(handle: &engine::plugin::PluginHandle) -> String {
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(mtm) = MainThreadMarker::new() else { return "no es el hilo principal".into() };
    // An application object that never shows up in the Dock.
    unsafe {
        if let Some(class) = objc2::runtime::AnyClass::get(c"NSApplication") {
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![class, sharedApplication];
            let _: bool = objc2::msg_send![app, setActivationPolicy: 2isize];
        }
    }
    let Some(mut view) = handle.editor() else { return "sin editor propio".into() };
    if !view.supports(engine::plugin::PLATFORM_VIEW) {
        return "su editor no admite NSView".into();
    }
    let (w, h) = view.size();
    let requested = std::rc::Rc::new(std::cell::Cell::new(None));
    {
        let requested = requested.clone();
        view.on_resize(move |w, h| requested.set(Some((w, h))));
    }
    let parent = NSView::initWithFrame(mtm.alloc(), NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w as f64, h as f64)));
    if !unsafe { view.attach(Retained::as_ptr(&parent) as *mut std::ffi::c_void) } {
        return format!("editor de {w}×{h} que no se pudo montar");
    }
    let subviews = parent.subviews().len();
    let resizable = view.can_resize();
    let resized = if resizable { view.set_size(w + 100, h + 60) } else { (w, h) };
    drop(view);
    let after = parent.subviews().len();
    format!(
        "editor de {w}×{h} montado ({subviews} vista{}), {}{}, desmontado ({after} vistas tras cerrarlo)",
        if subviews == 1 { "" } else { "s" },
        if resizable { format!("redimensionable: pide {}×{} → {}×{}", w + 100, h + 60, resized.0, resized.1) } else { "tamaño fijo".into() },
        requested.get().map_or(String::new(), |(w, h)| format!(", pidió {w}×{h}")),
    )
}

#[cfg(windows)]
fn check_editor(handle: &engine::plugin::PluginHandle) -> String {
    win::check_editor(handle)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn check_editor(_handle: &engine::plugin::PluginHandle) -> String {
    "solo se comprueba en macOS y Windows".into()
}

#[cfg(target_os = "macos")]
mod mac {
    use engine::plugin::{PluginHandle, PluginView};
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSBackingStoreType, NSFloatingWindowLevel, NSWindow, NSWindowDelegate, NSWindowStyleMask};
    use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
    use std::cell::Cell;
    use std::rc::Rc;

    pub struct DelegateState {
        closed: Rc<Cell<bool>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = DelegateState]
        struct WindowDelegate;

        unsafe impl NSObjectProtocol for WindowDelegate {}

        unsafe impl NSWindowDelegate for WindowDelegate {
            /// The user closed the window (hiding with the app does not count).
            #[unsafe(method(windowWillClose:))]
            fn window_will_close(&self, _notification: &NSNotification) {
                self.ivars().closed.set(true);
            }
        }
    );

    /// Whether hiding an editor window leaves it open and closing it does not: checked on
    /// a window that is never put on screen.
    pub fn check_closing() -> String {
        let Some(mtm) = MainThreadMarker::new() else { return "no es el hilo principal".into() };
        let rect = NSRect::new(NSPoint::new(-10_000.0, -10_000.0), NSSize::new(10.0, 10.0));
        let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), rect, NSWindowStyleMask::Titled | NSWindowStyleMask::Closable, NSBackingStoreType::Buffered, true) };
        unsafe { window.setReleasedWhenClosed(false) };
        let closed = Rc::new(Cell::new(false));
        let delegate = WindowDelegate::new(mtm, closed.clone());
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.orderOut(None);
        let after_hiding = closed.get();
        window.performClose(None);
        let after_closing = closed.get();
        window.setDelegate(None);
        format!("ocultar la ventana {} la cierra; cerrarla {} avisa", if after_hiding { "SÍ" } else { "no" }, if after_closing { "sí" } else { "NO" })
    }

    impl WindowDelegate {
        fn new(mtm: MainThreadMarker, closed: Rc<Cell<bool>>) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(DelegateState { closed });
            unsafe { msg_send![super(this), init] }
        }
    }

    pub struct Editor {
        // The view goes before the window: the editor leaves its window first.
        view: Option<PluginView>,
        window: Retained<NSWindow>,
        /// The window keeps only a weak reference to its delegate.
        _delegate: Retained<WindowDelegate>,
        closed: Rc<Cell<bool>>,
        /// The content size last given to the editor (or asked for by it).
        size: Rc<Cell<(f64, f64)>>,
    }

    impl Editor {
        pub fn open(handle: &PluginHandle, title: &str) -> Result<Editor, String> {
            let mtm = MainThreadMarker::new().ok_or("los editores solo se abren desde la ventana principal")?;
            let mut view = handle.editor().ok_or("no tiene editor propio; usa Parámetros…")?;
            if !view.supports(engine::plugin::PLATFORM_VIEW) {
                return Err("su editor no funciona en macOS".into());
            }
            let (width, height) = view.size();
            let (width, height) = (width.max(100) as f64, height.max(60) as f64);
            let mut style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Miniaturizable;
            if view.can_resize() {
                style |= NSWindowStyleMask::Resizable;
            }
            let rect = NSRect::new(NSPoint::new(160.0, 160.0), NSSize::new(width, height));
            let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(mtm.alloc(), rect, style, NSBackingStoreType::Buffered, false) };
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&NSString::from_str(title));
            window.setLevel(NSFloatingWindowLevel);
            window.setHidesOnDeactivate(true);
            window.center();
            let content = window.contentView().ok_or("ventana sin contenido")?;
            let size = Rc::new(Cell::new((width, height)));
            {
                let (window, size) = (window.clone(), size.clone());
                view.on_resize(move |w, h| {
                    let wanted = (w.max(1) as f64, h.max(1) as f64);
                    size.set(wanted);
                    window.setContentSize(NSSize::new(wanted.0, wanted.1));
                });
            }
            if !unsafe { view.attach(Retained::as_ptr(&content) as *mut std::ffi::c_void) } {
                window.close();
                return Err("su editor no se pudo abrir".into());
            }
            let closed = Rc::new(Cell::new(false));
            let delegate = WindowDelegate::new(mtm, closed.clone());
            window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            window.makeKeyAndOrderFront(None);
            Ok(Editor { view: Some(view), window, _delegate: delegate, closed, size })
        }

        pub fn front(&self) {
            // Also brings back a window something else hid.
            self.window.makeKeyAndOrderFront(None);
        }

        /// False once the user closed the window. Hands window resizes to the editor.
        pub fn poll(&self) -> bool {
            // Only closing counts: the window also hides while Pulse is in the background.
            if self.closed.get() {
                return false;
            }
            if let (Some(view), Some(content)) = (&self.view, self.window.contentView()) {
                let frame = content.frame();
                let now = (frame.size.width, frame.size.height);
                let (w, h) = self.size.get();
                if (now.0 - w).abs() >= 1.0 || (now.1 - h).abs() >= 1.0 {
                    let (w, h) = view.set_size(now.0 as i32, now.1 as i32);
                    let accepted = (w as f64, h as f64);
                    self.size.set(accepted);
                    if accepted != now {
                        self.window.setContentSize(NSSize::new(accepted.0, accepted.1));
                    }
                }
            }
            true
        }
    }

    impl Drop for Editor {
        fn drop(&mut self) {
            self.view.take();
            self.window.setDelegate(None);
            self.window.close();
        }
    }
}

#[cfg(windows)]
mod win {
    use engine::plugin::{PluginHandle, PluginView};
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::rc::Rc;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    const CLASS: &str = "PulsePluginEditor";
    /// Windows' "100 %".
    const BASE_DPI: u32 = 96;

    /// What the window procedure tells the editor (through GWLP_USERDATA).
    struct WindowState {
        closed: Cell<bool>,
        /// The DPI of the screen the window is on, once it moved to another.
        dpi: Cell<Option<u32>>,
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const WindowState;
        match message {
            // The user closed it: the editor goes on the next frame, and the window with it.
            WM_CLOSE if !state.is_null() => {
                unsafe { (*state).closed.set(true) };
                0
            }
            // Moved to a screen of another scale: take the size Windows suggests,
            // and tell the editor on the next frame.
            WM_DPICHANGED if !state.is_null() => {
                let suggested = unsafe { &*(lparam as *const RECT) };
                unsafe {
                    (*state).dpi.set(Some(((wparam >> 16) & 0xffff) as u32));
                    SetWindowPos(hwnd, std::ptr::null_mut(), suggested.left, suggested.top, suggested.right - suggested.left, suggested.bottom - suggested.top, SWP_NOZORDER | SWP_NOACTIVATE);
                }
                0
            }
            _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
        }
    }

    fn register() -> Result<(), String> {
        static REGISTERED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let ok = *REGISTERED.get_or_init(|| {
            let name = wide(CLASS);
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(procedure),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
                hIcon: std::ptr::null_mut(),
                hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            unsafe { RegisterClassExW(&class) != 0 }
        });
        if ok { Ok(()) } else { Err("no se pudo preparar la ventana del editor".into()) }
    }

    fn style(resizable: bool) -> u32 {
        let mut style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        if resizable {
            style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
        }
        style
    }

    /// The window's outer size for a content of `width` × `height` pixels.
    fn outer(hwnd: HWND, width: i32, height: i32) -> (i32, i32) {
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(BASE_DPI);
        let mut rect = RECT { left: 0, top: 0, right: width, bottom: height };
        unsafe { AdjustWindowRectExForDpi(&mut rect, style, 0, 0, dpi) };
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    fn content_size(hwnd: HWND) -> (i32, i32) {
        let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe { GetClientRect(hwnd, &mut rect) };
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    fn resize(hwnd: HWND, width: i32, height: i32) {
        let (w, h) = outer(hwnd, width, height);
        unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    /// A window for an editor, owned by `owner`, not shown yet.
    fn create(title: &str, owner: HWND, resizable: bool) -> Result<HWND, String> {
        register()?;
        let (class, title) = (wide(CLASS), wide(title));
        let hwnd = unsafe {
            CreateWindowExW(0, class.as_ptr(), title.as_ptr(), style(resizable), CW_USEDEFAULT, CW_USEDEFAULT, 400, 300, owner, std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null())
        };
        if hwnd.is_null() { Err("no se pudo crear la ventana del editor".into()) } else { Ok(hwnd) }
    }

    /// Centers `hwnd` over `owner` (or leaves it where Windows put it).
    fn center(hwnd: HWND, owner: HWND) {
        let (mut window, mut over) = (RECT { left: 0, top: 0, right: 0, bottom: 0 }, RECT { left: 0, top: 0, right: 0, bottom: 0 });
        if owner.is_null() || unsafe { GetWindowRect(hwnd, &mut window) } == 0 || unsafe { GetWindowRect(owner, &mut over) } == 0 {
            return;
        }
        let (w, h) = (window.right - window.left, window.bottom - window.top);
        let x = over.left + ((over.right - over.left) - w) / 2;
        let y = (over.top + ((over.bottom - over.top) - h) / 2).max(over.top);
        unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
    }

    pub struct Editor {
        // The view goes before the window: the editor leaves its window first.
        view: Option<PluginView>,
        hwnd: HWND,
        state: Box<WindowState>,
        /// The content size last given to the editor (or asked for by it), in pixels.
        size: Rc<Cell<(i32, i32)>>,
    }

    impl Editor {
        pub fn open(handle: &PluginHandle, title: &str) -> Result<Editor, String> {
            let mut view = handle.editor().ok_or("no tiene editor propio; usa Parámetros…")?;
            if !view.supports(engine::plugin::PLATFORM_VIEW) {
                return Err("su editor no funciona en Windows".into());
            }
            // Pulse's window, active when the editor is asked for: the editor stays over it.
            let owner = unsafe { GetActiveWindow() };
            let hwnd = create(title, owner, view.can_resize())?;
            let state = Box::new(WindowState { closed: Cell::new(false), dpi: Cell::new(None) });
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const WindowState as isize) };
            // Editors on Windows work in pixels: they're told the scale before saying their size.
            view.set_scale(unsafe { GetDpiForWindow(hwnd) }.max(BASE_DPI) as f64 / BASE_DPI as f64);
            let (width, height) = view.size();
            let (width, height) = (width.max(100), height.max(60));
            resize(hwnd, width, height);
            center(hwnd, owner);
            let size = Rc::new(Cell::new((width, height)));
            {
                let size = size.clone();
                view.on_resize(move |w, h| {
                    let wanted = (w.max(1), h.max(1));
                    size.set(wanted);
                    resize(hwnd, wanted.0, wanted.1);
                });
            }
            if !unsafe { view.attach(hwnd as *mut c_void) } {
                drop(view);
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    DestroyWindow(hwnd);
                }
                return Err("su editor no se pudo abrir".into());
            }
            unsafe {
                ShowWindow(hwnd, SW_SHOW);
                SetForegroundWindow(hwnd);
            }
            Ok(Editor { view: Some(view), hwnd, state, size })
        }

        pub fn front(&self) {
            unsafe {
                if IsIconic(self.hwnd) != 0 {
                    ShowWindow(self.hwnd, SW_RESTORE);
                } else {
                    ShowWindow(self.hwnd, SW_SHOW);
                }
                SetForegroundWindow(self.hwnd);
            }
        }

        /// False once the user closed the window. Hands window resizes and scale
        /// changes to the editor.
        pub fn poll(&self) -> bool {
            if self.state.closed.get() {
                return false;
            }
            let Some(view) = &self.view else { return true };
            if let Some(dpi) = self.state.dpi.take() {
                view.set_scale(dpi.max(BASE_DPI) as f64 / BASE_DPI as f64);
            }
            // Minimized windows have no content to follow.
            if unsafe { IsIconic(self.hwnd) } != 0 {
                return true;
            }
            let now = content_size(self.hwnd);
            if now != self.size.get() && now.0 > 0 && now.1 > 0 {
                let accepted = view.set_size(now.0, now.1);
                self.size.set(accepted);
                if accepted != now {
                    resize(self.hwnd, accepted.0, accepted.1);
                }
            }
            true
        }
    }

    impl Drop for Editor {
        fn drop(&mut self) {
            self.view.take();
            unsafe {
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(self.hwnd);
            }
        }
    }

    /// Whether closing an editor's window is noticed and hiding it is not: checked on
    /// a window that is never shown.
    pub fn check_closing() -> String {
        let Ok(hwnd) = create("Pulse", std::ptr::null_mut(), false) else { return "no se pudo crear una ventana".into() };
        let state = Box::new(WindowState { closed: Cell::new(false), dpi: Cell::new(None) });
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const WindowState as isize) };
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        let after_hiding = state.closed.get();
        unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };
        let after_closing = state.closed.get();
        let alive = unsafe { IsWindow(hwnd) } != 0;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            DestroyWindow(hwnd);
        }
        format!(
            "ocultar la ventana {} la cierra; cerrarla {} avisa{}",
            if after_hiding { "SÍ" } else { "no" },
            if after_closing { "sí" } else { "NO" },
            if alive { "" } else { " (y la destruye antes de tiempo)" }
        )
    }

    /// Mounts an installed plugin's editor in a window that is never shown, resizes
    /// it and takes it out again.
    pub fn check_editor(handle: &PluginHandle) -> String {
        let Some(mut view) = handle.editor() else { return "sin editor propio".into() };
        if !view.supports(engine::plugin::PLATFORM_VIEW) {
            return "su editor no admite HWND".into();
        }
        let resizable = view.can_resize();
        let Ok(hwnd) = create("Pulse", std::ptr::null_mut(), resizable) else { return "no se pudo crear una ventana".into() };
        let scaled = view.set_scale(1.0);
        let (w, h) = view.size();
        resize(hwnd, w.max(1), h.max(1));
        let requested = Rc::new(Cell::new(None));
        {
            let requested = requested.clone();
            view.on_resize(move |w, h| requested.set(Some((w, h))));
        }
        if !unsafe { view.attach(hwnd as *mut c_void) } {
            unsafe { DestroyWindow(hwnd) };
            return format!("editor de {w}×{h} que no se pudo montar");
        }
        let child = !unsafe { GetWindow(hwnd, GW_CHILD) }.is_null();
        let resized = if resizable { view.set_size(w + 100, h + 60) } else { (w, h) };
        drop(view);
        let left = !unsafe { GetWindow(hwnd, GW_CHILD) }.is_null();
        unsafe { DestroyWindow(hwnd) };
        format!(
            "editor de {w}×{h} montado ({}), {}{}{}, desmontado ({})",
            if child { "con su ventana" } else { "sin ventana propia" },
            if resizable { format!("redimensionable: pide {}×{} → {}×{}", w + 100, h + 60, resized.0, resized.1) } else { "tamaño fijo".into() },
            if scaled { ", acepta la escala" } else { "" },
            requested.get().map_or(String::new(), |(w, h)| format!(", pidió {w}×{h}")),
            if left { "dejó su ventana" } else { "sin restos" },
        )
    }
}
