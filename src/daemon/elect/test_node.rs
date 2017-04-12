//! Tests for the case of single node
//!
use super::{Message};
use super::action::Action;
use super::machine::Machine;
use super::test_util::{Environ};

fn ping() -> Message {
    Message::Ping
}

#[test]
fn test_starting() {
    let env = Environ::new("beef01");
    let node: Machine = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));
}

#[test]
fn test_alone() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    env.sleep(100);  // Small time, just continue starting
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    assert!(act.action == None);
    env.sleep(10000);  // Large timeout, should already become a leader
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Leader { .. }));
    assert!(act.action == Some(Action::PingAll));
}

#[test]
fn test_start_vote() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));

    env.add_node();
    env.sleep(10000);  // Large timeout, should start_election
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { .. }));
    assert!(act.action == Some(Action::Vote(env.id.clone())));
}

#[test]
fn test_vote_approved() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    let id = env.id.clone();
    assert!(matches!(node, Machine::Starting { .. }));

    let two = env.add_node();
    env.sleep(10000);  // Large timeout, should start_election
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { .. }));
    assert!(act.action == Some(Action::Vote(id.clone())));

    let (node, act) = node.message(&env.info(),
        (two.clone(), 1, Message::Vote(id.clone())), env.now());
    assert!(matches!(node, Machine::Leader { .. }));
    assert!(act.action == Some(Action::PingAll));
}

#[test]
fn test_election_expired() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    let id = env.id.clone();
    assert!(matches!(node, Machine::Starting { .. }));

    env.add_node();
    env.sleep(10000);  // Large timeout, should start_election
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { epoch: 1, .. }));
    assert!(act.action == Some(Action::Vote(id.clone())));

    env.sleep(3000);  // Large timeout, should be enough for new election
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { epoch: 2, .. }));
    assert!(act.action == Some(Action::Vote(id.clone())));
}

#[test]
fn test_voted_timeout() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));

    env.tick();
    let two = env.add_node();
    let (node, act) = node.message(&env.info(),
        (two.clone(), 1, Message::Vote(two.clone())), env.now());
    assert!(act.action == Some(Action::ConfirmVote(two.clone())));
    assert!(matches!(node, Machine::Voted { .. }));

    env.sleep(4000);  // Large timeout, should be enough for new election
    let (node, _) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { epoch: 2, .. }));
}

#[test]
fn test_leader_timeout() {
    // this block is same as in test_alone (optimize
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    env.sleep(100);  // Small time, just continue starting
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    assert!(act.action == None);
    env.sleep(10000);  // Large timeout, should already become a leader
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Leader { .. }));
    assert!(act.action == Some(Action::PingAll));
    // end of copy'n'paste

    env.sleep(3000);  // Large timeout, should make a ping
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Leader { .. }));
    assert!(act.action == Some(Action::PingAll));
}

#[test]
fn test_follower_timeout() {
    let mut env = Environ::new("beef01");
    let id = env.id.clone();
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    env.tick();
    let two = env.add_node();
    let (node, act) = node.message(&env.info(),
        (two.clone(), 1, ping()), env.now());
    assert!(matches!(node, Machine::Follower { .. }));
    assert!(matches!(act.action, Some(Action::Pong(..))));

    env.sleep(4000);  // Large timeout, should start new election
    let (node, act) = node.time_passed(&env.info(), env.now());
    assert!(matches!(node, Machine::Electing { .. }));
    assert!(act.action == Some(Action::Vote(id.clone())));
}

#[test]
fn test_voted_ping() {
    let mut env = Environ::new("beef01");
    let node = Machine::new(env.now());
    assert!(matches!(node, Machine::Starting { .. }));
    env.tick();

    let two = env.add_node();
    let (node, act) = node.message(&env.info(),
        (two.clone(), 1, Message::Vote(two.clone())), env.now());
    assert!(act.action == Some(Action::ConfirmVote(two.clone())));
    assert!(matches!(node, Machine::Voted { .. }));

    let (node, act) = node.message(&env.info(),
        (two.clone(), 1, ping()), env.now());
    assert!(matches!(node, Machine::Follower { .. }));
    assert!(matches!(act.action, Some(Action::Pong(..))));
}
