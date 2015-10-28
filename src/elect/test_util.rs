use time::{SteadyTime, Timespec, Duration, get_time};
use std::sync::atomic::{AtomicUsize, Ordering, ATOMIC_USIZE_INIT};
use std::net;

use super::{Info, Id, PeerInfo};

static NODE_COUNTER: AtomicUsize = ATOMIC_USIZE_INIT;


pub struct Environ {
    now: SteadyTime,
    tspec: Timespec,
}

impl Environ {
    pub fn new() -> Environ {
        Environ {
            // unfortunately we can't create arbitrary steady time value
            now: SteadyTime::now(),
            tspec: get_time(),
        }
    }
    pub fn sleep(&mut self, ms: i64) {
        self.now = self.now +  Duration::milliseconds(ms);
        self.tspec = self.tspec +  Duration::milliseconds(ms);
    }
    pub fn now(&self) -> SteadyTime {
        self.now
    }
    pub fn add_another_for(&self, info: &mut Info) -> Id {
        let n = NODE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let id = Id(format!("ext_node_{}", n));
        info.all_hosts.insert(id.clone(), PeerInfo {
            addr: net::SocketAddr::V4(net::SocketAddrV4::new(
                net::Ipv4Addr::new(127, 0, (n >> 8) as u8, (n & 0xFF) as u8),
                12345)),
            last_report: self.tspec,
        });
        id
    }
}
