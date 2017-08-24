extern crate libc;
extern crate futures;

use std::fs::File;
use std::any::Any;
use std::path::Path;
use std::io::Write;
use std::io::BufReader;
use std::io::BufRead;
use std::ffi::CString;
use std::net::TcpStream;
use std::net::SocketAddr;
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

// used to stop its learner
pub struct LearnerHandle {
    th: thread::JoinHandle<()>,
    tid: pthread_t,
}

impl LearnerHandle {
    // Stop its learner
    pub fn stop(self) -> Result<(), Box<Any + Send + 'static>> {
        unsafe {
            pthread_kill(self.tid, SIGINT);
        }
        self.th.join()
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
                 })
                 .unwrap();
        });
    });
    let tid = recv.wait().unwrap();
    (DecisionStream { inner: drecv }, LearnerHandle { th: th, tid: tid })
}

// Connection to a proposer.
pub struct ProposerConnection {
    pub pid: i64,
    conn: TcpStream,
}

impl ProposerConnection {
    pub fn submit<'a, V: Into<&'a [u8]>>(&mut self, value: V) -> Result<(), std::io::Error> {
        let bytes = value.into();
        let len = bytes.len();
        let ptr = bytes.as_ptr() as *const c_char;
        let mut vec = vec![];
        let vec_ptr: *mut Vec<u8> = &mut vec;
        unsafe { wrapper::serialize_submit(ptr, len, writer_write::<Vec<u8>>, vec_ptr as *mut c_void) };
        self.conn.write_all(&vec)
    }
}

fn proposer_address(config: &Path, pid: i64) -> Result<SocketAddr, std::io::Error> {
    let f = File::open(config)?;
    let bf = BufReader::new(f);
    for line in bf.lines() {
        match line {
            Ok(line) => {
                let line = line.trim();
                if line.starts_with("replica") || line.starts_with("proposer") {
                    let parts: Vec<_> = line.split_whitespace().collect();
                    // FIXME: crashes on malformed config
                    let id: i64 = parts[1].parse().unwrap();
                    if id == pid {
                        let mut addrstr = String::new();
                        addrstr = addrstr + parts[2] + ":" + parts[3];
                        return Ok(addrstr.parse().unwrap())
                    }
                }
            }
            Err(err) => return Err(err)
        }
    }
    // FIXME: panics if proposer id not found
    panic!("proposer id not found on config file")
}

// Connect to the proposer with the given Id. The returned
// ProposerConnection can be used to submit commands to Paxos.
pub fn connect_to_proposer(config: &Path, pid: i64) -> ProposerConnection {
    // FIXME: unwraps
    let addr = proposer_address(config, pid).unwrap();
    let conn = TcpStream::connect(addr).unwrap();
    conn.set_nodelay(true).unwrap();
    ProposerConnection {
        pid: pid,
        conn: conn,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
