extern crate libpaxos;
extern crate futures;

use std::path::Path;
use futures::*;


fn main() {
    let config = Path::new("paxos.conf");
    let (decisions, _lh) = libpaxos::start_learner_as_stream(config, 0);

    let mut next_decision = decisions.into_future();
    while let Ok((Some(decision), decisions)) = next_decision.wait() {
        println!("iid: {} value: {}", decision.iid, String::from_utf8(decision.value).unwrap());
        next_decision = decisions.into_future();
    }
}
