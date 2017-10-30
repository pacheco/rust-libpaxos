extern crate libc;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_service;

use std::io::Write;
use std::io::BufReader;
use std::net::SocketAddr;
use std::slice;

use libc::size_t;

pub mod proposer;
pub mod learner;
pub use errors::*;

mod errors {
    use std;
    use futures;

    pub type Result<T> = std::result::Result<T, Error>;

    pub enum Error {
        Io(std::io::Error),
        FutureCanceled(futures::Canceled),
        Other(String),
    }

    impl From<std::io::Error> for Error {
        fn from(other: std::io::Error) -> Self {
            Error::Io(other)
        }
    }

    impl From<futures::Canceled> for Error {
        fn from(other: futures::Canceled) -> Self {
            Error::FutureCanceled(other)
        }
    }

    impl From<String> for Error {
        fn from(other: String) -> Self {
            Error::Other(other)
        }
    }

    pub fn other_err<E: std::fmt::Debug>(other: E) -> Error {
        Error::Other(format!("{:?}", other))
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
        pub fn start_learner(config: *const c_char, cb: DeliverFn, arg: *mut c_void, starting_iid: c_uint);
        pub fn serialize_submit(value: *const c_char,
                                len: size_t,
                                write_fn: SerializeWriteFn,
                                write_arg: *mut c_void);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
