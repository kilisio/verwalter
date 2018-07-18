use std::collections::HashSet;

use futures::{Stream, Async, Future};
use futures::sync::mpsc;
use futures::future::{loop_fn, Loop};

use tk_easyloop::spawn;
use frontend::incoming::{Subscription, Incoming};


#[derive(Clone, Debug)]
pub struct Sender(mpsc::UnboundedSender<Subscription>);

#[derive(Debug)]
pub struct Receiver(mpsc::UnboundedReceiver<Subscription>);

pub fn new() -> (Sender, Receiver) {
    let (tx, rx) = mpsc::unbounded();
    return (Sender(tx), Receiver(rx));
}

impl Sender {
    pub fn trigger(&self, subscription: Subscription) {
        self.0.unbounded_send(subscription)
            .map_err(|e| error!("Can't trigger subscription: {}", e))
            .ok();
    }
}

impl Receiver {
    pub fn start(self, inc: &Incoming) {
        let inc = inc.clone();
        let Receiver(me) = self;
        let me = me.fuse();
        spawn(loop_fn((inc, me), move |(inc, me)| {
            me.into_future()
            .map_err(|((), _stream)| {
                error!("Subscription sender closed");
            })
            .and_then(move |(item, mut me)| {
                let first = match item {
                    None => return Ok(Loop::Break(())),
                    Some(item) => item,
                };
                let mut buf = HashSet::new();
                buf.insert(first);
                loop {
                    match me.poll() {
                        Err(e) => return Err(e),
                        Ok(Async::Ready(Some(x))) => {
                            buf.insert(x);
                        }
                        Ok(Async::Ready(None)) => break,
                        Ok(Async::NotReady) => break,
                    }
                }
                for item in buf {
                    inc.trigger(&item);
                }
                Ok(Loop::Continue((inc, me)))
            })
        }))
    }
}
