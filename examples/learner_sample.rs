extern crate libpaxos;
extern crate libc;
extern crate futures;

use libc::*;
use futures::*;

use std::path::Path;
use std::slice;
use std::ffi::CStr;

#[repr(C)]
struct ClientValue {
    client_id: c_int,
    t: timeval,
    size: size_t,
    value: [u8; 0],
}

// Learner printing decisions from libpaxos sample client.
pub fn sample_client_learner() {
    let config = Path::new("paxos.conf");
    let (decisions, _lh) = libpaxos::learner::decision_stream(config, 0);
    // std::thread::spawn(move || {
    //     thread::sleep_ms(10000);
    //     println!("sigint to learner...");
    //     lh.stop().unwrap();
    // });

    let mut next_decision = decisions.into_future();
    while let Ok((Some(decision), decisions)) = next_decision.wait() {
        let cval = unsafe { &*(decision.value.as_ptr() as *const ClientValue) };
        let bytes = unsafe { slice::from_raw_parts(cval.value.as_ptr(), cval.size) };
        let c_str = unsafe { CStr::from_bytes_with_nul_unchecked(bytes) };
        // println!("iid: {:?} cid: {:?} val: {:?}",
        //          decision.iid,
        //          cval.client_id,
        //          c_str);
        next_decision = decisions.into_future();
    }
    println!("learner done!");
}

fn main() {
    sample_client_learner();
}
