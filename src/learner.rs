use ::*;

use std::thread;
use std::slice;
use std::ffi::CString;
use std::path::Path;

use futures::*;
use futures::sync::oneshot;
use futures::sync::mpsc;

use libc::pthread_t;
use libc::pthread_self;
use libc::pthread_kill;
use libc::c_uint;
use libc::c_char;
use libc::size_t;
use libc::c_void;
use libc::SIGINT;


// Rust side wrapper
// -----------------------

extern "C" fn deliver_cb<F>(iid: c_uint, value: *const c_char, len: size_t, arg: *mut c_void)
    where F: FnMut(u64, &[u8])
{
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    let callback = unsafe { &mut *(arg as *mut F) };
    callback(iid as u64, bytes);
}

fn run_learner<F>(config: &Path, starting_iid: u64, mut deliver_fn: F)
    where F: FnMut(u64, &[u8])
{
    let c_cfg = CString::new(config.to_str().unwrap()).unwrap();
    let cb_ptr = &mut deliver_fn as *mut F as *mut c_void;
    unsafe {
        wrapper::start_learner(c_cfg.as_ptr(), deliver_cb::<F>, cb_ptr, starting_iid as c_uint);
    }
}

// High-level interface
// ----------------------

// used to stop its learner
pub struct LearnerHandle {
    th: thread::JoinHandle<()>,
    tid: pthread_t,
}

impl LearnerHandle {
    // Stop its learner
    pub fn stop(self) -> Result<()> {
        unsafe {
            pthread_kill(self.tid, SIGINT);
        }
        self.th.join().map_err(|_| other_err("error joining thread"))
    }
}

// The stream of Paxos decisions
pub struct DecisionStream {
    inner: mpsc::UnboundedReceiver<Decision>,
}

#[derive(Debug)]
// Decided paxos slot
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

// Start a learner. It will run in its own thread and decisions are sent to the returned stream.
// The handle can be used to stop the learner.
pub fn decision_stream(config: &Path, starting_iid: u64) -> (DecisionStream, LearnerHandle) {
    // learner evloop runs in a background thread. To kill it, we
    // issue a pthread_kill, the C wrapper handles the SIGKILL to
    // shutdown the evloop.
    let config = config.to_owned();
    let (send, recv) = oneshot::channel();
    let (dsend, drecv) = mpsc::unbounded();
    let th = thread::spawn(move || {
        send.send(unsafe { pthread_self() }).unwrap();
        run_learner(&config, starting_iid, |iid, bytes| {
            dsend.unbounded_send(Decision {
                     iid: iid,
                     value: Vec::from(bytes),
                 })
                 .unwrap();
        });
    });
    let tid = recv.wait().unwrap();
    (DecisionStream { inner: drecv }, LearnerHandle { th: th, tid: tid })
}

// Start a learner. It will run in its own thread and decision_fn will be called for each decision.
pub fn start_learner<F>(config: &Path, starting_iid: u64, decision_fn: F) -> LearnerHandle
    where F: FnMut(u64, &[u8]) + Send + 'static
{
    let config = config.to_owned();
    let (send, recv) = oneshot::channel();
    let th = thread::spawn(move || {
        send.send(unsafe { pthread_self() }).unwrap();
        run_learner(&config, starting_iid, decision_fn);
    });
    let tid = recv.wait().unwrap();
    LearnerHandle { th: th, tid: tid }
}
