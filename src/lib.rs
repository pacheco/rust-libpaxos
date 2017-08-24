extern crate libc;

use std::path::Path;
use std::io::Write;
use std::ffi::CString;
use std::ffi::CStr;
use std::slice;

use libc::timeval;
use libc::c_int;
use libc::c_char;
use libc::c_void;
use libc::c_uint;
use libc::size_t;

// C interfacing

type DeliverFn = extern "C" fn(c_uint, *const c_char, size_t, *mut c_void);
type SerializeWriteFn = extern "C" fn(*mut c_void, *const c_char, size_t);

#[link(name = "evpaxos")]
#[link(name = "event")]
extern "C" {
    fn start_learner(config: *const c_char, cb: DeliverFn, arg: *mut c_void);
    fn serialize_submit(value: *const c_char,
                        len: size_t,
                        write_fn: SerializeWriteFn,
                        write_arg: *mut c_void);
}

extern "C" fn writer_write<T: Write>(write: *mut c_void, value: *const c_char, len: size_t) {
    let write = unsafe { &mut *(write as *mut T) };
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    write.write_all(bytes).unwrap();
}

// High level interface

struct Callback<F, A> {
    f: F,
    arg: A,
}

extern "C" fn deliver_cb<F, A>(iid: c_uint,
                               value: *const c_char,
                               len: size_t,
                               arg: *mut c_void)
    where F: Fn(&mut A, u64, &[u8])
{
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    let callback = unsafe { &mut* (arg as *mut Callback<F,A>) };
    let f = &callback.f;
    let arg = &mut callback.arg;
    f(arg, iid as u64, bytes);
}

pub fn run_learner<F, A>(config: &Path, deliver_ctx: A, deliver_fn: F)
    where F: Fn(&mut A, u64, &[u8])
{
    let c_cfg = CString::new(config.to_str().unwrap()).unwrap();
    let mut arg = Callback {
        f: deliver_fn,
        arg: deliver_ctx,
    };
    let arg_ptr = &mut arg as *mut Callback<F,A> as *mut c_void;
    unsafe {
        start_learner(c_cfg.as_ptr(), deliver_cb::<F,A>, arg_ptr);
    }
}

// extern "C" fn test_cb(iid: c_uint, data: *const c_char, _size: size_t, _arg: *mut c_void) {
//     let msg = unsafe { CStr::from_ptr(data) };
//     println!("delivered {}: {:?}", iid, msg);
// }

#[repr(C)]
struct ClientValue {
    client_id: c_int,
    t: timeval,
    size: size_t,
    value: [u8;0],
}

pub fn test_serialize() {
    let data = CString::new("foobar").unwrap();
    let len = data.as_bytes().len();
    let ptr = data.as_ptr();
    let mut vec = vec![];
    let vec_ptr: *mut Vec<u8> = &mut vec;
    unsafe { serialize_submit(ptr, len, writer_write::<Vec<u8>>, vec_ptr as *mut c_void) };
    println!("{:?}", vec);
}

pub fn test() {
    let config = Path::new("paxos.conf");
    run_learner(&config, 0, |ctx, iid, bytes| {
        let cval = unsafe { &*(bytes.as_ptr() as *const ClientValue) };
        let bytes = unsafe { slice::from_raw_parts(cval.value.as_ptr(), cval.size) };
        let c_str = unsafe { CStr::from_bytes_with_nul_unchecked(bytes) };
        println!("{:?} {:?} {:?} {:?}", ctx, iid, cval.client_id, c_str);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
