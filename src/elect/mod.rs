use std::net::SocketAddr;
use std::collections::{HashSet, HashMap};

use time::SteadyTime;
use time::Timespec;

mod machine;
mod action;
mod settings;
mod info;
#[cfg(test)] mod test_node;
#[cfg(test)] mod test_mesh;
#[cfg(test)] mod test_util;
#[cfg(test)] mod test_split_brain;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Id(String);

type Capsule = (u64, Message);

#[derive(Clone, Debug)]
enum Machine {
    Starting { leader_deadline: SteadyTime },
    Electing { votes_for_me: HashSet<Id>, election_deadline: SteadyTime },
    Voted { peer: Id, election_deadline: SteadyTime },
    Leader { ping_time: SteadyTime },
    Follower { leader_deadline: SteadyTime },
}

#[derive(Clone, Debug)]
enum Message {
    /// Ping message from leader to followers, reassures that leadership
    /// still holds
    Ping,
    /// Pong message from follower to leader, confirm that node is a leader
    Pong,
    /// Vote for some node
    Vote(Id),
}

#[derive(Clone, Debug)]
struct PeerInfo {
     addr: SocketAddr,
     last_report: Timespec,
}

#[derive(Debug)]
struct Info {
    /// Unique identificator of the node, should be read from /etc/machine-id
    id: Id,
    /// State machine of the leader election
    all_hosts: HashMap<Id, PeerInfo>,
}

impl ::std::fmt::Display for Id {
    fn fmt(&self, fmt: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        write!(fmt, "{}", self.0)
    }
}
