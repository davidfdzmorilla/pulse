//! Projects macOS asks Pulse to open: a double clic in Finder, "Abrir con", or a
//! file dropped on the Dock icon. They arrive as `application:openURLs:` on the
//! application delegate, which belongs to the windowing library and doesn't
//! answer it, so the method is added to its class as the app starts. The paths
//! wait in a queue that the app empties every frame.

use std::path::PathBuf;
use std::sync::Mutex;

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CONTEXT: std::sync::OnceLock<eframe::egui::Context> = std::sync::OnceLock::new();

/// Paths macOS asked to open since the last call.
pub fn take() -> Vec<PathBuf> {
    OPENED.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

fn push(path: PathBuf) {
    if let Ok(mut queue) = OPENED.lock() {
        queue.push(path);
    }
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// Lets a file opened while Pulse runs wake the window up.
pub fn set_context(ctx: &eframe::egui::Context) {
    let _ = CONTEXT.set(ctx.clone());
}

#[cfg(target_os = "macos")]
pub use mac::listen;

#[cfg(not(target_os = "macos"))]
pub fn listen() {}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{class, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSString};

    define_class!(
        // SAFETY: NSObject has no subclassing requirements, and this class has no Drop.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        struct LaunchObserver;

        impl LaunchObserver {
            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _notification: &NSNotification) {
                answer_open_urls();
            }
        }

        unsafe impl NSObjectProtocol for LaunchObserver {}
    );

    /// Call before the event loop starts: when the application is about to finish
    /// launching (its delegate is set by then, and files to open come right after),
    /// the delegate learns to take them.
    pub fn listen() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let observer = LaunchObserver::alloc(mtm).set_ivars(());
        let observer: Retained<LaunchObserver> = unsafe { msg_send![super(observer), init] };
        let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
        unsafe {
            let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![&center, addObserver: &*observer, selector: sel!(willFinishLaunching:), name: &*name, object: std::ptr::null::<AnyObject>()];
        }
        // Lives as long as the app.
        std::mem::forget(observer);
    }

    fn answer_open_urls() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&app, delegate] };
        let Some(delegate) = delegate else { return };
        let class = unsafe { objc2::ffi::object_getClass(&*delegate) } as *mut AnyClass;
        let method: unsafe extern "C-unwind" fn(&AnyObject, Sel, *const AnyObject, &AnyObject) = open_urls;
        // SAFETY: the types string matches the function: void, self, _cmd, two objects.
        unsafe {
            let imp: Imp = std::mem::transmute(method);
            objc2::ffi::class_addMethod(class, sel!(application:openURLs:), imp, c"v@:@@".as_ptr());
        }
    }

    unsafe extern "C-unwind" fn open_urls(_this: &AnyObject, _cmd: Sel, _app: *const AnyObject, urls: &AnyObject) {
        let count: usize = unsafe { msg_send![urls, count] };
        for i in 0..count {
            let url: Retained<AnyObject> = unsafe { msg_send![urls, objectAtIndex: i] };
            let path: Option<Retained<NSString>> = unsafe { msg_send![&url, path] };
            if let Some(path) = path {
                super::push(path.to_string().into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_paths_wait_in_order_until_taken() {
        push("/a.pulse".into());
        push("/b.als".into());
        assert_eq!(take(), vec![PathBuf::from("/a.pulse"), PathBuf::from("/b.als")]);
        assert!(take().is_empty());
    }
}
