extern crate libpaxos;

use std::path::Path;

fn main() {
    let mut p = libpaxos::connect_to_proposer(&Path::new("paxos.conf"), 0);
    p.submit("Hello".as_bytes()).unwrap();
    p.submit("World".as_bytes()).unwrap();
    p.submit("Foobar".as_bytes()).unwrap();
}
