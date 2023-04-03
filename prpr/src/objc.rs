pub use ::objc::{
    declare::ClassDecl,
    rc::StrongPtr,
    runtime::{Class, Object, Sel, NO, YES},
    *,
};
pub use block::ConcreteBlock;
pub use objc_foundation::{INSArray, INSString, NSArray, NSObject, NSString};
pub use objc_id::{Id, Owned, ShareId, Shared};

use once_cell::sync::Lazy;

pub type ObjcId = *mut Object;

pub fn str_to_ns(s: impl AsRef<str>) -> Id<NSString> {
    NSString::from_str(s.as_ref())
}

pub fn available(version: &str) -> bool {
    static SYSTEM_VERSION: Lazy<String> = Lazy::new(|| unsafe {
        let device: ObjcId = msg_send![class!(UIDevice), currentDevice];
        let version: &NSString = msg_send![device, systemVersion];
        version.as_str().to_owned()
    });
    version
        .chars()
        .filter(|it| it.is_ascii_digit())
        .le(SYSTEM_VERSION.chars().filter(|it| it.is_ascii_digit()))
}
