pub use block2::{Block, RcBlock};
pub use objc2::rc::Retained;
pub use objc2::runtime;
pub use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
pub use objc2::{class, msg_send, sel};
pub use objc2_foundation::{NSArray, NSObject, NSString};

use once_cell::sync::Lazy;

pub type ObjcId = *mut AnyObject;

pub fn str_to_ns(s: impl AsRef<str>) -> Retained<NSString> {
    NSString::from_str(s.as_ref())
}

pub fn available(version: &str) -> bool {
    static SYSTEM_VERSION: Lazy<String> = Lazy::new(|| unsafe {
        let device: ObjcId = msg_send![class!(UIDevice), currentDevice];
        let version: *mut NSString = msg_send![device, systemVersion];
        let version = &*version;
        version.to_string()
    });
    version
        .chars()
        .filter(|it| it.is_ascii_digit())
        .le(SYSTEM_VERSION.chars().filter(|it| it.is_ascii_digit()))
}
