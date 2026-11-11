//! "Brink x.y.z is ready" as a macOS notification. Clicking it restarts Brink into the new
//! version. Permission is asked the first time an update is actually ready, not at launch.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, ClassType};
use objc2_foundation::{NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::OnceLock;

const UPDATE_ID: &str = "brink-update";

static ON_CLICK: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Delegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "BrinkNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &DynBlock<dyn Fn()>) {
            let id = response.notification().request().identifier();
            if id.to_string() == UPDATE_ID {
                if let Some(f) = ON_CLICK.get() {
                    f();
                }
            }
            done.call(());
        }

        // Show the banner even though Brink counts as the active app while its panel is open.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            done: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }
    }
);

/// Installs the click handler. Call once at launch, so a click on a notification that is
/// still sitting in Notification Center works too.
pub fn init(on_click: impl Fn() + Send + Sync + 'static) {
    if ON_CLICK.set(Box::new(on_click)).is_err() {
        return;
    }
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::class(), new] };
    UNUserNotificationCenter::currentNotificationCenter().setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly; Brink keeps this one for its whole life.
    std::mem::forget(delegate);
}

pub fn update_ready(version: &str) {
    let title = format!("Brink {version} is ready");
    let ask = RcBlock::new(move |granted: Bool, _err: *mut NSError| {
        if !granted.as_bool() {
            log::info!("brink: notifications not allowed, update waits for the next restart");
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str("Click to restart Brink into the new version."));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(UPDATE_ID), &content, None);
        UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &ask);
}
