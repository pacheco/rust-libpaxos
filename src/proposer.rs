use ::*;

use std::fs::File;
use std::path::Path;
use std::io::BufRead;
use std::marker::PhantomData;

use futures::*;
use futures::sync::oneshot;
use futures::sync::mpsc;

use tokio_core::reactor::Handle;
use tokio_core::net::TcpStream;

use tokio_io;
use tokio_service::Service;

use libc::c_char;
use libc::c_void;


// Connection to a proposer.
pub struct ProposerClient<V> {
    pub pid: i64,
    submit_tx: mpsc::UnboundedSender<(Vec<u8>, oneshot::Sender<Result<()>>)>,
    phantom: PhantomData<V>,
}

pub struct Submitted {
    inner: Option<oneshot::Receiver<Result<()>>>,
}

impl Submitted {
    fn new(inner: Option<oneshot::Receiver<Result<()>>>) -> Self {
        Submitted { inner: inner }
    }
}

impl Future for Submitted {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<(), Error> {
        match self.inner {
            Some(ref mut result_rx) => {
                match result_rx.poll()? {
                    Async::Ready(Ok(())) => Ok(Async::Ready(())),
                    Async::Ready(Err(err)) => Err(err),
                    Async::NotReady => Ok(Async::NotReady),
                }
            }
            None => Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe").into()),
        }
    }
}

impl<V: AsRef<[u8]>> ProposerClient<V> {
    pub fn submit(&self, value: V) -> Submitted {
        self.call(value)
    }
}

impl<V: AsRef<[u8]>> Service for ProposerClient<V> {
    type Request = V;
    type Response = ();
    type Error = Error;
    type Future = Submitted;

    fn call(&self, value: V) -> Self::Future {
        let len = value.as_ref().len();
        let ptr = value.as_ref().as_ptr() as *const c_char;
        let mut vec = vec![];
        let vec_ptr: *mut Vec<u8> = &mut vec;
        unsafe {
            wrapper::serialize_submit(ptr, len, writer_write::<Vec<u8>>, vec_ptr as *mut c_void)
        };
        let (done_tx, done_rx) = oneshot::channel();
        match self.submit_tx.unbounded_send((vec, done_tx)) {
            Ok(_) => Submitted::new(Some(done_rx)),
            Err(_) => Submitted::new(None),
        }
    }
}

impl<V: 'static> ProposerClient<V> {
    pub fn connect(config: &Path,
                   pid: i64,
                   handle: &Handle)
                   -> Box<Future<Item = Self, Error = Error>> {
        let (tx, rx) = mpsc::unbounded::<(Vec<u8>, oneshot::Sender<Result<()>>)>();
        let (conntx, connrx) = oneshot::channel::<Result<()>>();

        // background task connecting to proposer and submitting values
        let addr = proposer_address(config, pid);
        let connect = {
            let handle = handle.clone();
            future::done(addr).and_then(move |addr| {
                TcpStream::connect(&addr, &handle).map_err(|err| err.into())
            })
        };

        let submit_loop = connect.then(move |result| {
            match result {
                Ok(sock) => {
                    conntx.send(Ok(())).ok(); // ignore
                    println!("connected to proposer {}", pid);
                    Box::new(rx.fold(sock, |sock, (val, done_tx)| {
                        println!("submiting a value");
                        tokio_io::io::write_all(sock, val)
                            .then(|result| {
                                match result {
                                    Ok((sock,_)) => {
                                        done_tx.send(Ok(())).ok(); // ignore
                                        Ok(sock)
                                    }
                                    Err(err) => {
                                        done_tx.send(Err(err.into())).ok(); //ignore
                                        Err(())
                                    }
                                }
                            })
                    })) as Box<Future<Item=_, Error=_>>
                }
                Err(err) => {
                    println!("could not connect to proposer {}", pid);
                    conntx.send(Err(err)).ok(); // ignore
                    Box::new(Err(()).into_future())
                }
            }
        });
        handle.spawn(submit_loop.map(|_| ()));

        Box::new(connrx.map_err(|err| err.into()).and_then(move |c| {
            if let Err(e) = c {
                Err(e)
            } else {
                Ok(ProposerClient {
                    pid: pid,
                    submit_tx: tx,
                    phantom: PhantomData,
                })
            }
        }))
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

// callback called by C code to write bytes into a Rust Vec
extern "C" fn writer_write<T: Write>(write: *mut c_void, value: *const c_char, len: size_t) {
    let write = unsafe { &mut *(write as *mut T) };
    let bytes = unsafe { slice::from_raw_parts(value as *mut u8, len) };
    write.write_all(bytes).unwrap();
}
