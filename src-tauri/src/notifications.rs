//! System notifications. A packaged macOS app uses the UserNotifications framework, which
//! has a real permission prompt and status; the older API behind the notification plugin no
//! longer delivers on recent macOS. Elsewhere (Windows, and `tauri dev`, which is not an app
//! bundle) the plugin posts and the permission state cannot be read.

use serde::Serialize;
use tauri::AppHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStatus {
    Unknown,
    NotDetermined,
    Denied,
    Authorized,
}

/// Lets banners show while the window is in front and clicks open the window.
pub fn install(_app: &AppHandle) {
    #[cfg(target_os = "macos")]
    mac::install(_app);
}

pub fn status() -> NotificationStatus {
    #[cfg(target_os = "macos")]
    if let Some(status) = mac::status() {
        return status;
    }
    NotificationStatus::Unknown
}

/// Shows the system's permission prompt if the user has not answered it yet.
pub fn request() -> NotificationStatus {
    #[cfg(target_os = "macos")]
    if mac::available() {
        return mac::request();
    }
    NotificationStatus::Unknown
}

pub fn post(app: &AppHandle, title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    if mac::post(title, body) {
        return;
    }
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Posts after a pause. macOS keeps a notification from the frontmost app out of the desktop
/// (it only lands in Notification Center) unless the user turned Notification Center off for
/// it, so notices sent from the window wait until another app is likely in front.
pub fn post_later(app: &AppHandle, delay: std::time::Duration, title: String, body: String) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        post(&app, &title, &body);
    });
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ptr::NonNull;
    use std::sync::{mpsc, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::NotificationStatus;

    static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "TokenAuditNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                _response: &UNNotificationResponse,
                handler: &block2::DynBlock<dyn Fn()>,
            ) {
                if let Some(app) = APP.get() {
                    crate::tray::show_main(app);
                }
                handler.call(());
            }
        }
    );

    /// Asking a bare executable (`tauri dev`) for the notification center aborts the process.
    pub fn available() -> bool {
        let bundle = NSBundle::mainBundle();
        bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
    }

    pub fn install(app: &tauri::AppHandle) {
        if !available() {
            return;
        }
        let _ = APP.set(app.clone());
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc().set_ivars(());
            unsafe { msg_send![super(this), init] }
        };
        UNUserNotificationCenter::currentNotificationCenter()
            .setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The center holds its delegate weakly; it has to live as long as the app.
        std::mem::forget(delegate);
    }

    pub fn status() -> Option<NotificationStatus> {
        if !available() {
            return None;
        }
        let (sender, receiver) = mpsc::channel();
        let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let _ = sender.send(unsafe { settings.as_ref() }.authorizationStatus());
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&block);
        let status = receiver.recv_timeout(Duration::from_secs(5)).ok()?;
        Some(match status {
            UNAuthorizationStatus::NotDetermined => NotificationStatus::NotDetermined,
            UNAuthorizationStatus::Denied => NotificationStatus::Denied,
            _ => NotificationStatus::Authorized,
        })
    }

    pub fn request() -> NotificationStatus {
        let (sender, receiver) = mpsc::channel();
        let block = RcBlock::new(move |_granted: Bool, error: *mut NSError| {
            // An app whose signature does not match its bundle id is refused without a prompt.
            if let Some(error) = unsafe { error.as_ref() } {
                eprintln!(
                    "notification authorization failed: {}",
                    error.localizedDescription()
                );
            }
            let _ = sender.send(());
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &block,
            );
        // The user may take a while to answer the prompt.
        let _ = receiver.recv_timeout(Duration::from_secs(300));
        status().unwrap_or(NotificationStatus::Unknown)
    }

    pub fn post(title: &str, body: &str) -> bool {
        if !available() {
            return false;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&format!("token-audit-{stamp}")),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
        true
    }
}
