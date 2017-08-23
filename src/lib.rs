extern crate libc;

use std::ffi::CStr;
use std::ffi::CString;

use libc::c_char;
use libc::c_void;
use libc::c_uint;
use libc::size_t;

#[link(name = "evpaxos")]
#[link(name = "event")]
extern {
    fn start_learner(config: *const c_char,
                     cb: extern fn(c_uint, *const c_char, size_t, *const c_void),
                     arg: *const c_void);
}

extern fn test_cb(iid: c_uint, data: *const c_char, _size: size_t, _arg: *const c_void) {
    let msg = unsafe { CStr::from_ptr(data) };
    println!("delivered {}: {:?}", iid, msg);
}

pub fn test() {
    let config = CString::new("paxos.conf").unwrap();
    let arg = 0 as *const c_void;
    unsafe {
        start_learner(config.as_ptr(), test_cb, arg);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
