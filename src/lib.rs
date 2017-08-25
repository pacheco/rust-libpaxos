extern crate libc;
extern crate futures;
#[macro_use]
extern crate error_chain;
extern crate tokio_core;
extern crate tokio_io;

use std::fs::File;
use std::path::Path;
use std::io::Write;
use std::io::BufReader;
use std::io::BufRead;
use std::ffi::CString;
use std::net::SocketAddr;
use std::slice;
use std::thread;
use std::rc::Rc;
use std::cell::RefCell;

use futures::*;
use futures::sync::oneshot;
use futures::sync::mpsc;

use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;

use libc::pthread_t;
use libc::pthread_self;
use libc::pthread_kill;
use libc::c_char;
use libc::c_void;
use libc::c_uint;
use libc::size_t;
use libc::SIGINT;

pub use errors::*;

mod errors {
    use std;
    use futures;

    error_chain! {
        errors {
            ProposerClientError(value: Vec<u8>) {
            }
        }

        foreign_links {
            Io(std::io::Error);
            FutureCanceled(futures::Canceled);
        }
    }
}


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

extern "C" fn deliver_cb<F>(iid: c_uint, value: *const c_char, len: size_t, arg: *mut c_void)
    where F: FnMut(u64, &[u8])
{
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    let callback = unsafe { &mut *(arg as *mut F) };
    callback(iid as u64, bytes);
}

fn run_learner<F>(config: &Path, mut deliver_fn: F)
    where F: FnMut(u64, &[u8])
{
    let c_cfg = CString::new(config.to_str().unwrap()).unwrap();
    let cb_ptr = &mut deliver_fn as *mut F as *mut c_void;
    unsafe {
        wrapper::start_learner(c_cfg.as_ptr(), deliver_cb::<F>, cb_ptr);
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
        self.th.join().map_err(|_| "error joining thread".into())
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
pub fn start_learner_as_stream(config: &Path) -> (DecisionStream, LearnerHandle) {
    // learner evloop runs in a background thread. To kill it, we
    // issue a pthread_kill, the C wrapper handles the SIGKILL to
    // shutdown the evloop.
    let config = config.to_owned();
    let (send, recv) = oneshot::channel();
    let (dsend, drecv) = mpsc::unbounded();
    let th = thread::spawn(move || {
        send.send(unsafe { pthread_self() }).unwrap();
        run_learner(&config, |iid, bytes| {
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
pub fn start_learner<F>(config: &Path, decision_fn: F) -> LearnerHandle
    where F: FnMut(u64, &[u8]) + Send + 'static
{
    let config = config.to_owned();
    let (send, recv) = oneshot::channel();
    let th = thread::spawn(move || {
        send.send(unsafe { pthread_self() }).unwrap();
        run_learner(&config, decision_fn);
    });
    let tid = recv.wait().unwrap();
    LearnerHandle { th: th, tid: tid }
}


// Connection to a proposer.
pub struct ProposerClient {
    pub pid: i64,
    submit_tx: mpsc::UnboundedSender<Vec<u8>>,
}


impl ProposerClient {
    pub fn connect(config: &Path,
                   pid: i64,
                   handle: &Handle)
                   -> Box<Future<Item = Self, Error = Error>> {
        let (tx, rx) = mpsc::unbounded::<Vec<u8>>();
        let (conntx, connrx) = oneshot::channel::<Result<()>>();

        // background task connecting to proposer and submitting values
        let addr = proposer_address(config, pid);
        let connect = {
            let handle = handle.clone();
            future::done(addr).and_then(move |addr| {
                TcpStream::connect(&addr, &handle).map_err(|err| err.into())
            })
        };

        let conntx = Rc::new(RefCell::new(Some(conntx)));
        let conntx2 = conntx.clone();
        let submit_loop =
            connect.or_else(move |err| {
                       println!("could not connect to proposer {}", pid);
                       conntx.borrow_mut().take().unwrap().send(Err(err)).ok(); // ignore
                       Err(())
                   })
                   .and_then(move |sock| {
                       conntx2.borrow_mut().take().unwrap().send(Ok(())).ok(); // ignore
                       println!("connected to proposer {}", pid);
                       rx.fold(sock, |sock, val| {
                           println!("submiting a value");
                           tokio_io::io::write_all(sock, val)
                               .map_err(|_| ())
                               .map(|(sock, _)| sock)
                       })
                   });
        handle.spawn(submit_loop.map(|_| ()));

        Box::new(connrx.map_err(|err| err.into()).and_then(move |c| {
            if let Err(e) = c {
                Err(e)
            } else {
                Ok(ProposerClient {
                    pid: pid,
                    submit_tx: tx,
                })
            }
        }))
    }

    pub fn submit<V: AsRef<[u8]>>(&self, value: V) -> Result<()> {
        let len = value.as_ref().len();
        let ptr = value.as_ref().as_ptr() as *const c_char;
        let mut vec = vec![];
        let vec_ptr: *mut Vec<u8> = &mut vec;
        unsafe {
            wrapper::serialize_submit(ptr, len, writer_write::<Vec<u8>>, vec_ptr as *mut c_void)
        };
        self.submit_tx
            .unbounded_send(vec)
            .map_err(|err| ErrorKind::ProposerClientError(err.into_inner()).into())
    }
}


fn proposer_address(config: &Path, pid: i64) -> Result<SocketAddr> {
    let f = File::open(config)?;;;;;;;;;;
    let bf = BufReader::new(f);
    for line in bf.lines() {
        let line = line?;;;;;;;
        let line = line.trim();
        if line.starts_with("replica") || line.starts_with("proposer") {
            let parts: Vec<_> = line.split_whitespace().collect();
            // FIXME: crashes on malformed config
            let id: i64 = parts[1].parse().unwrap();
            if id == pid {
                let mut addrstr = String::new();
                addrstr = addrstr + parts[2] + ":" + parts[3];
                return Ok(addrstr.parse().unwrap());
            }
        }
    }
    // FIXME: panics if proposer id not found
    panic!("proposer id not found on config file")
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
