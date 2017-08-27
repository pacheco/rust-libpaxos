extern crate libc;
extern crate futures;
#[macro_use]
extern crate error_chain;
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
