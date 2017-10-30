extern crate futures;
extern crate tokio_core;
extern crate tokio_timer;
extern crate tokio_service;
extern crate libpaxos;

use futures::*;

use tokio_core::reactor::Core;

use tokio_service::Service;

use tokio_timer::Timer;

use std::time::Duration;
use std::path::Path;

fn main() {
    let mut core = Core::new().unwrap();
    let handle = core.handle();
    let connect = libpaxos::proposer::ProposerClient::connect(&Path::new("paxos.conf"), 0, &handle);
    let to_send = stream::iter_ok(vec![
        "Hello",
        "World",
        "Foobar",
    ]);
    // let to_send = stream::repeat("Hello");
    let submit_stuff = connect.and_then(move |p| {
        to_send.for_each(move |s| {
            p.call(s)
        })
    }).and_then(|_| {
        Timer::default().sleep(Duration::from_secs(5)).map_err(|_| "timer failed".into())
    });
    core.run(submit_stuff).unwrap();
}
