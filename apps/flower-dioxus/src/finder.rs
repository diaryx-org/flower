//! The files the system asks the app to open: a double-click in the Finder,
//! Open With, a file dropped on the Dock icon, `open -a Flower config.toml`.
//!
//! On a Mac those arrive as an Apple Event, not on the command line, and
//! AppKit hands them to the application delegate's `application:openURLs:` —
//! so the app installs a delegate of its own before the event loop starts.
//! winit promises never to register one (`winit-appkit`'s crate docs), so it
//! is the app's to take. The delegate only queues the paths; the document's
//! window takes them from [`take`], on the same poll that takes the menu's
//! choices, and opens them as ⌘O would.
//!
//! Elsewhere a file is opened from the command line, and this is a queue
//! nothing fills.

use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    /// The paths asked for and not yet opened. AppKit calls the delegate on
    /// the main thread, which is the thread the page runs on.
    static ASKED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// The files asked for since the last call.
pub fn take() -> Vec<PathBuf> {
    ASKED.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

/// Install the application delegate. Called once, from `main`, before the
/// event loop starts: the Apple Event for the file a launch was asked to
/// open comes in as the app finishes launching.
pub fn install() {
    #[cfg(target_os = "macos")]
    mac::install();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{NSApplication, NSApplicationDelegate};
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};

    use super::ASKED;

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "FlowerAppDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl NSApplicationDelegate for Delegate {
            #[unsafe(method(application:openURLs:))]
            fn application_open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
                let paths: Vec<PathBuf> = urls
                    .iter()
                    .filter_map(|url| url.path())
                    .map(|p| PathBuf::from(p.to_string()))
                    .collect();
                ASKED.with(|a| a.borrow_mut().extend(paths));
            }
        }
    );

    impl Delegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            unsafe { msg_send![super(Self::alloc(mtm).set_ivars(())), init] }
        }
    }

    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        let delegate = Delegate::new(mtm);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The application holds its delegate weakly; this one lives as long
        // as the app.
        std::mem::forget(delegate);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_asked_for_is_taken_once() {
        ASKED.with(|a| a.borrow_mut().push(PathBuf::from("/tmp/a.toml")));
        assert_eq!(take(), vec![PathBuf::from("/tmp/a.toml")]);
        assert!(take().is_empty());
    }
}
