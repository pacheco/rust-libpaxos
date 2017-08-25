extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate libpaxos;

use futures::*;

use tokio_core::reactor::Core;
use tokio_timer::Timer;

use std::time::Duration;
use std::path::Path;

fn main() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let connect = libpaxos::ProposerClient::connect(&Path::new("paxos.conf"), 0, &handle);
    let submit_stuff = connect.and_then(|p| {
        p.submit("Hello".as_bytes()).unwrap();
        p.submit("World".as_bytes()).unwrap();
        p.submit("Foobar".as_bytes()).unwrap();
        Timer::default().sleep(Duration::from_secs(5)).map_err(|_| "timer failed".into())
    });
    core.run(submit_stuff).unwrap();
}
