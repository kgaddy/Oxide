//! Files Finder sends to Oxide: double-clicking a .oxide painting, "Open
//! With → Oxide", or dropping files on Oxide's Dock icon.
//!
//! macOS delivers these as an "open documents" Apple Event, not as program
//! arguments, and winit (the window library) doesn't pass it on. So we
//! register our own handler with AppKit's Apple Event manager. AppKit
//! installs its own default handlers while the app finishes launching, so
//! ours goes in when AppKit announces "will finish launching" (Apple's
//! recommended moment: a double-click that *starts* Oxide isn't missed),
//! and again once the app is running. The paths wait in a queue that the
//! app takes each frame.

use std::path::PathBuf;
use std::sync::Mutex;

static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static WAKE: Mutex<Option<eframe::egui::Context>> = Mutex::new(None);

/// Files that arrived since the last call.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *QUEUE.lock().expect("open-files queue"))
}

/// Let arriving files wake the app up (call once the app exists).
pub fn set_wake(ctx: &eframe::egui::Context) {
    *WAKE.lock().expect("open-files wake") = Some(ctx.clone());
    #[cfg(target_os = "macos")]
    mac::register_handler();
}

#[cfg(target_os = "macos")]
fn arrived(paths: Vec<PathBuf>) {
    QUEUE.lock().expect("open-files queue").extend(paths);
    if let Some(ctx) = &*WAKE.lock().expect("open-files wake") {
        ctx.request_repaint();
    }
}

/// Start listening for "open documents" events (call at the start of `main`).
#[cfg(target_os = "macos")]
pub fn install() {
    mac::install();
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::PathBuf;
    use std::ptr::NonNull;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{AnyThread, class, define_class, msg_send, sel};
    use objc2_foundation::NSString;

    /// Apple Event codes (four-character codes).
    const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
    const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    define_class!(
        // An object AppKit can call back: it only has the event-handler method.
        #[unsafe(super(NSObject))]
        #[name = "OxideOpenDocumentsHandler"]
        struct Handler;

        impl Handler {
            #[unsafe(method(handleOpenDocuments:withReply:))]
            fn handle_open_documents(&self, event: &AnyObject, _reply: &AnyObject) {
                super::arrived(paths_in(event));
            }
        }
    );

    /// The file paths in an "open documents" event.
    fn paths_in(event: &AnyObject) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        // SAFETY: `event` is an NSAppleEventDescriptor; these are its documented methods.
        unsafe {
            let list: Option<Retained<AnyObject>> = msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT];
            let Some(list) = list else { return paths };
            let count: isize = msg_send![&*list, numberOfItems];
            for i in 1..=count {
                let item: Option<Retained<AnyObject>> = msg_send![&*list, descriptorAtIndex: i];
                let Some(item) = item else { continue };
                // The item as a file URL (macOS converts aliases and paths for us).
                let url: Option<Retained<AnyObject>> = msg_send![&*item, fileURLValue];
                let Some(url) = url else { continue };
                let path: Option<Retained<NSString>> = msg_send![&*url, path];
                if let Some(path) = path {
                    paths.push(PathBuf::from(path.to_string()));
                }
            }
        }
        paths
    }

    /// Register `register_handler` to run when AppKit is about to finish launching.
    pub fn install() {
        let block = RcBlock::new(|_note: NonNull<AnyObject>| register_handler());
        // SAFETY: NSNotificationCenter's block-based observer API; the
        // observer token and block are kept for the whole run (leaked).
        unsafe {
            let center: Retained<AnyObject> = msg_send![class!(NSNotificationCenter), defaultCenter];
            let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
            let token: Option<Retained<AnyObject>> = msg_send![
                &*center,
                addObserverForName: &*name,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
            std::mem::forget(token);
        }
        std::mem::forget(block);
    }

    /// Make our object the handler for "open documents" events.
    pub fn register_handler() {
        let handler: Retained<Handler> = unsafe { msg_send![Handler::alloc(), init] };
        // SAFETY: standard NSAppleEventManager registration; the handler is
        // kept alive for the whole run (leaked below).
        unsafe {
            let manager: Retained<AnyObject> = msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
            let _: () = msg_send![
                &*manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleOpenDocuments:withReply:),
                forEventClass: CORE_EVENT_CLASS,
                andEventID: OPEN_DOCUMENTS
            ];
        }
        std::mem::forget(handler);
    }
}
