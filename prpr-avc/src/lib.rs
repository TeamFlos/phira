mod ffi;

mod common;
pub use common::*;

mod avformat;
pub use avformat::*;

mod codec;
pub use codec::*;

mod frame;
pub use frame::*;

mod packet;
pub use packet::*;

mod stream;
pub use stream::*;

mod sws;
pub use sws::*;

mod video;
pub use video::*;

#[repr(transparent)]
struct OwnedPtr<T>(pub *mut T);
impl<T> OwnedPtr<T> {
    pub fn new(ptr: *mut T) -> Option<Self> {
        if ptr.is_null() {
            None
        } else {
            Some(Self(ptr))
        }
    }

    pub unsafe fn as_ref(&self) -> &T {
        std::mem::transmute(self.0)
    }

    pub unsafe fn as_mut(&mut self) -> &mut T {
        std::mem::transmute(self.0)
    }

    pub fn as_self_mut(&mut self) -> *mut *mut T {
        &mut self.0
    }
}

fn handle(code: ::std::os::raw::c_int) -> AVResult<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(AVError::new(code))
    }
}
