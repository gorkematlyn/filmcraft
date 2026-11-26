//! macOS: the Finder's "open documents" request (a double-click, Open With, or a file dropped on
//! the Dock icon), for FilmCraft closed and already running.
//!
//! macOS does not start an application with the file on its command line. It sends the running
//! application an `application:openURLs:` message to its delegate, and when the delegate cannot
//! answer, AppKit shows "The document … could not be opened. FilmCraft cannot open files in the
//! “FilmCraft Project” format." The delegate that winit installs implements only the quit and
//! launch messages, and winit does not let an application replace it.
//!
//! [`install`] therefore adds that one method to the class of the delegate that is installed, in
//! the notification AppKit posts just before it delivers the launch-time open request. The method
//! hands the paths to the callback given to [`install`]. Nothing else of the delegate changes.
//!
//! This is the only module of the crate that is not media FFI; ADR 0001 records the exception.
//! Rules as for the other FFI modules: every `unsafe` block has a `// SAFETY:` comment, and no
//! panic unwinds into AppKit (the method runs under `catch_unwind`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSApplicationWillFinishLaunchingNotification};
use objc2_foundation::{NSArray, NSNotification, NSNotificationCenter, NSURL};

type Handler = Box<dyn Fn(Vec<String>) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Call `handler` with the file paths macOS asks the application to open, on the main thread.
/// Call once, on the main thread, before the event loop starts (the request of a double-click that
/// launched the application arrives while it starts). A second call does nothing.
pub fn install(handler: impl Fn(Vec<String>) + Send + Sync + 'static) {
    if HANDLER.set(Box::new(handler)).is_err() {
        return;
    }
    let block = RcBlock::new(|_: NonNull<NSNotification>| add_method());
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the name is AppKit's own notification name, `object` and `queue` are None (any
    // sender, delivered on the posting thread), and the block captures nothing and is `Fn`.
    let token = unsafe { center.addObserverForName_object_queue_usingBlock(Some(NSApplicationWillFinishLaunchingNotification), None, None, &block) };
    // The observer stays for the life of the process.
    std::mem::forget(token);
}

/// Add `application:openURLs:` to the class of the application's delegate, unless it has one.
fn add_method() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        log::warn!("no application delegate: files opened from the Finder will not reach FilmCraft");
        return;
    };
    let object: &AnyObject = delegate.as_ref();
    let class: &AnyClass = object.class();
    let selector = sel!(application:openURLs:);
    if class.responds_to(selector) {
        return;
    }
    let method: extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>) = open_urls;
    // SAFETY: `Imp` is the type-erased form of a method implementation; `open_urls` takes the
    // receiver, the selector and the two object arguments the type encoding below describes.
    let imp = unsafe { std::mem::transmute::<extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, &NSArray<NSURL>), Imp>(method) };
    // SAFETY: `class` is a live class, the selector is not one of its methods (checked above), the
    // encoding `v@:@@` is void return, receiver, selector, two objects, as `open_urls` is.
    let added = unsafe { class_addMethod(std::ptr::from_ref(class).cast_mut(), selector, imp, c"v@:@@".as_ptr()) };
    if !added.as_bool() {
        log::warn!("could not add application:openURLs: to the application delegate: files opened from the Finder will not reach FilmCraft");
    }
}

/// `application:openURLs:`. Only file URLs are taken; their paths are decoded (spaces, accents,
/// any character), so they are the paths the file system uses.
extern "C-unwind" fn open_urls(_this: &AnyObject, _cmd: Sel, _application: &AnyObject, urls: &NSArray<NSURL>) {
    let paths = catch_unwind(AssertUnwindSafe(|| {
        let mut paths = Vec::new();
        for i in 0..urls.count() {
            let url = urls.objectAtIndex(i);
            if url.isFileURL()
                && let Some(path) = url.path()
            {
                paths.push(path.to_string());
            }
        }
        paths
    }))
    .unwrap_or_default();
    if let Some(handler) = HANDLER.get().filter(|_| !paths.is_empty()) {
        let _ = catch_unwind(AssertUnwindSafe(|| handler(paths)));
    }
}
