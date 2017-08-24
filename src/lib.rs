extern crate libc;
extern crate futures;

use std::any::Any;
use std::path::Path;
use std::io::Write;
use std::ffi::CString;
use std::ffi::CStr;
use std::slice;
use std::thread;

use futures::Future;
use futures::Stream;
use futures::Poll;
use futures::sync::oneshot;
use futures::sync::mpsc;

use libc::pthread_t;
use libc::pthread_self;
use libc::pthread_kill;
use libc::timeval;
use libc::c_int;
use libc::c_char;
use libc::c_void;
use libc::c_uint;
use libc::size_t;
use libc::SIGINT;

// C wrapper
// ---------------------

mod wrapper {
    use libc::c_char;
    use libc::c_void;
    use libc::c_uint;
    use libc::size_t;

    pub type DeliverFn = extern "C" fn(c_uint, *const c_char, size_t, *mut c_void);
    pub type SerializeWriteFn = extern "C" fn(*mut c_void, *const c_char, size_t);

    #[link(name = "evpaxos")]
    #[link(name = "event")]
    extern "C" {
        pub fn start_learner(config: *const c_char, cb: DeliverFn, arg: *mut c_void);
        pub fn serialize_submit(value: *const c_char,
                                len: size_t,
                                write_fn: SerializeWriteFn,
                                write_arg: *mut c_void);
    }
}

extern "C" fn writer_write<T: Write>(write: *mut c_void, value: *const c_char, len: size_t) {
    let write = unsafe { &mut *(write as *mut T) };
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    write.write_all(bytes).unwrap();
}

// Rust side wrapper
// -----------------------

struct Callback<F, A> {
    f: F,
    arg: A,
}

extern "C" fn deliver_cb<F, A>(iid: c_uint, value: *const c_char, len: size_t, arg: *mut c_void)
    where F: Fn(&mut A, u64, &[u8])
{
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    let callback = unsafe { &mut *(arg as *mut Callback<F, A>) };
    let f = &callback.f;
    let arg = &mut callback.arg;
    f(arg, iid as u64, bytes);
}

fn run_learner<F, A>(config: &Path, deliver_ctx: A, deliver_fn: F)
    where F: Fn(&mut A, u64, &[u8])
{
    let c_cfg = CString::new(config.to_str().unwrap()).unwrap();
    let mut arg = Callback {
        f: deliver_fn,
        arg: deliver_ctx,
    };
    let arg_ptr = &mut arg as *mut Callback<F, A> as *mut c_void;
    unsafe {
        wrapper::start_learner(c_cfg.as_ptr(), deliver_cb::<F, A>, arg_ptr);
    }
}

// High-level interface
// ----------------------

pub struct LearnerHandle {
    th: thread::JoinHandle<()>,
    tid: pthread_t,
}

impl LearnerHandle {
    pub fn stop(self) -> Result<(), Box<Any + Send + 'static>> {
        unsafe { pthread_kill(self.tid, SIGINT); }
        self.th.join()
    }
}

pub struct DecisionStream {
    inner: mpsc::UnboundedReceiver<Decision>,
}

#[derive(Debug)]
pub struct Decision {
    pub iid: u64,
    pub value: Vec<u8>,
}

impl Stream for DecisionStream {
    type Item = Decision;
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}

pub fn start_learner(config: &Path) -> (DecisionStream, LearnerHandle) {
    let (send, recv) = oneshot::channel();
    // learner evloop runs in a background thread. To kill it, we
    // issue a pthread_kill, the C wrapper handles the SIGKILL to
    // shutdown the evloop.
    let config = config.to_owned();
    let (dsend, drecv) = mpsc::unbounded();
    let th = thread::spawn(move || {
        send.send(unsafe { pthread_self() }).unwrap();
        run_learner(&config, dsend, |dsend, iid, bytes| {
            dsend.send(Decision {
                iid: iid,
                value: Vec::from(bytes),
            }).unwrap();
        });
    });
    let tid = recv.wait().unwrap();
    (DecisionStream {
        inner: drecv,
    },
     LearnerHandle {
         th: th,
         tid: tid,
     })
}

// Ad-hoc tests

pub fn test_serialize() {
    let data = CString::new("foobar").unwrap();
    let len = data.as_bytes().len();
    let ptr = data.as_ptr();
    let mut vec = vec![];
    let vec_ptr: *mut Vec<u8> = &mut vec;
    unsafe { wrapper::serialize_submit(ptr, len, writer_write::<Vec<u8>>, vec_ptr as *mut c_void) };
    println!("{:?}", vec);
}

#[repr(C)]
struct ClientValue {
    client_id: c_int,
    t: timeval,
    size: size_t,
    value: [u8; 0],
}

// Learner printing decisions from libpaxos sample client
pub fn sample_client_learner() {
    let config = Path::new("paxos.conf");
    let (decisions, lh) = start_learner(config);
    thread::spawn(move || {
        thread::sleep_ms(2000);
        println!("sigint to learner...");
        lh.stop().unwrap();
    });

    let mut next_decision = decisions.into_future();
    while let Ok((Some(decision), decisions)) = next_decision.wait() {
        let cval = unsafe { &*(decision.value.as_ptr() as *const ClientValue) };
        let bytes = unsafe { slice::from_raw_parts(cval.value.as_ptr(), cval.size) };
        let c_str = unsafe { CStr::from_bytes_with_nul_unchecked(bytes) };
        println!("iid: {:?} cid: {:?} val: {:?}", decision.iid, cval.client_id, c_str);
        next_decision = decisions.into_future();
    }
    println!("learner done!");
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
