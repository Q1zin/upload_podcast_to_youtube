use crate::shared::handler::HostHandler;
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_methods,
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject},
    sel,
};
use objc2_app_kit::{
    NSApp, NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSEvent,
    NSUserInterfaceValidations, NSValidatedUserInterfaceItem,
};
use objc2_foundation::{NSBundle, NSObjectNSThreadPerformAdditions, ns_string};
use std::{cell::Cell, ptr};

define_class! {
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    pub struct HostAppDelegate;

    impl HostAppDelegate {
        #[unsafe(method(createApplication:))]
        unsafe fn create_application(&self, _object: Option<&AnyObject>) {
            let app = NSApp(MainThreadMarker::new().expect("Not running on the main thread"));
            assert!(app.isKindOfClass(HostApplication::class()));
            assert!(
                app.delegate()
                    .unwrap()
                    .isKindOfClass(HostAppDelegate::class())
            );

            let main_bundle = NSBundle::mainBundle();
            let _: Bool = msg_send![&main_bundle,
                loadNibNamed: ns_string!("MainMenu"),
                owner: &*app,
                topLevelObjects: ptr::null_mut::<*const AnyObject>()
            ];
        }
    }

    unsafe impl NSObjectProtocol for HostAppDelegate {}

    unsafe impl NSApplicationDelegate for HostAppDelegate {
        #[unsafe(method(applicationShouldTerminate:))]
        unsafe fn application_should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            NSApplicationTerminateReply::TerminateNow
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        unsafe fn application_should_handle_reopen(&self, _sender: &NSApplication, _has_visible_windows: Bool) -> Bool {
            Bool::NO
        }

        #[unsafe(method(applicationSupportsSecureRestorableState:))]
        unsafe fn application_supports_secure_restorable_state(&self, _sender: &NSApplication) -> Bool {
            Bool::YES
        }
    }

    unsafe impl NSUserInterfaceValidations for HostAppDelegate {
        #[unsafe(method(validateUserInterfaceItem:))]
        unsafe fn validate_user_interface_item(&self, item: &ProtocolObject<dyn NSValidatedUserInterfaceItem>) -> Bool {
            const IDC_FIND: isize = 37000;
            let tag = item.tag();
            if tag == IDC_FIND { Bool::YES } else { Bool::NO }
        }
    }
}

impl HostAppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = HostAppDelegate::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

#[derive(Default)]
pub struct HostApplicationIvars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    #[unsafe(super(NSApplication))]
    #[ivars = HostApplicationIvars]
    pub struct HostApplication;

    impl HostApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was_sending_event = self.is_handling_send_event();
            if !was_sending_event {
                self.set_handling_send_event(true);
            }

            let _: () = msg_send![super(self), sendEvent:event];

            if !was_sending_event {
                self.set_handling_send_event(false);
            }
        }

        #[unsafe(method(terminate:))]
        unsafe fn terminate(&self, _sender: &AnyObject) {
            if let Some(handler) = HostHandler::instance() {
                let mut handler = handler.lock().expect("Failed to lock HostHandler");
                if !handler.is_closing() {
                    handler.close_all_browsers(false);
                }
            }
        }
    }

    unsafe impl CrAppControlProtocol for HostApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn _set_handling_send_event(&self, handling_send_event: Bool) {
            self.ivars().handling_send_event.set(handling_send_event);
        }
    }

    unsafe impl CrAppProtocol for HostApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn _is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CefAppProtocol for HostApplication {}
);

impl HostApplication {
    extern_methods! {
        #[unsafe(method(sharedApplication))]
        fn shared_application() -> Retained<Self>;

        #[unsafe(method(setHandlingSendEvent:))]
        fn set_handling_send_event(&self, handling_send_event: bool);

        #[unsafe(method(isHandlingSendEvent))]
        fn is_handling_send_event(&self) -> bool;
    }
}

pub fn setup_host_application() {
    let _ = HostApplication::shared_application();
    assert!(
        NSApp(MainThreadMarker::new().expect("Not running on the main thread"))
            .isKindOfClass(HostApplication::class())
    );
}

pub fn setup_host_app_delegate() -> Retained<HostAppDelegate> {
    let mtm = MainThreadMarker::new().expect("Not running on the main thread");

    let host_delegate = HostAppDelegate::new(mtm);
    let delegate_proto =
        ProtocolObject::<dyn NSApplicationDelegate>::from_retained(host_delegate.clone());
    let app = NSApp(MainThreadMarker::new().expect("Not running on the main thread"));
    assert!(app.isKindOfClass(HostApplication::class()));
    app.setDelegate(Some(&delegate_proto));
    assert!(
        app.delegate()
            .unwrap()
            .isKindOfClass(HostAppDelegate::class())
    );

    unsafe {
        host_delegate.performSelectorOnMainThread_withObject_waitUntilDone(
            sel!(createApplication:),
            None,
            false,
        );
    }

    host_delegate
}
