
use std::collections::{HashSet, HashMap};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::borrow::Borrow;

use futures::Stream;
use futures::stream::MapErr;
use futures::sync::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use graphql_parser::query::OperationDefinition as Op;
use graphql_parser::query::{Definition, Document, Query as QueryParams};
use serde_json::{to_string};
use tk_bufstream::{WriteFramed, ReadFramed};
use tk_easyloop::handle;
use tk_http::websocket::{Loop, ServerCodec, Config, Packet};
use tokio_io::{AsyncRead, AsyncWrite};

use frontend::graphql;
use frontend::graphql::{Input, Context};
use frontend::dispatcher::{self, OutputMessage};


lazy_static! {
    static ref WEBSOCK_CONFIG: Arc<Config> = Config::new()
        .done();
    static ref CONNECTION_ID: AtomicUsize = AtomicUsize::new(0);
}

// TODO(tailhook) change to u64 when atomics are fixed
#[derive(Debug, Eq, PartialEq, Clone, Copy, Hash)]
struct Id(usize);

pub struct Token {
    holder: Incoming,
    key: Id,
}

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
pub enum Subscription {
}

#[derive(Debug)]
struct ConnImpl {
    id: Id,
    tx: UnboundedSender<Packet>,
    state: Mutex<ConnState>,
}
#[derive(Debug)]
struct ConnState {
    // request_id -> query
    subscriptions: HashMap<Subscription, HashMap<String, Input>>,
}

#[derive(Debug, Clone)]
pub struct Connection(Arc<ConnImpl>);

#[derive(Clone)]
pub struct Incoming(Arc<IncomingImpl>);

struct IncomingImpl {
    context: Context,
    state: Mutex<IncomingState>,
}

#[derive(Debug)]
struct IncomingState {
    connections: HashSet<Connection>,
    subscriptions: HashMap<Subscription, HashSet<Connection>>,
}

fn closed(():()) -> &'static str {
    "channel closed"
}

impl Incoming {
     pub fn new(context: &graphql::Context) -> Incoming {
        Incoming(Arc::new(IncomingImpl {
            context: context.clone(),
            state: Mutex::new(IncomingState {
                connections: HashSet::new(),
                subscriptions: HashMap::new(),
            }),
        }))
     }
     pub fn connected<S>(&self,
               output: WriteFramed<S, ServerCodec>,
               input: ReadFramed<S, ServerCodec>,
               graphql: &graphql::Context)
        -> (Token, Loop<S,
            MapErr<UnboundedReceiver<Packet>, fn(()) -> &'static str>,
            dispatcher::Dispatcher>)
        where S: AsyncRead + AsyncWrite,
     {
        let (tx, rx) = unbounded();
        let rx = rx.map_err(closed as fn(()) -> &'static str);
        let conn = {
            let mut lock = self.0.state.lock().expect("lock is not poisoned");
            let ref mut conns = lock.connections;
            let id = loop {
                let id = Id(CONNECTION_ID.fetch_add(1, Ordering::Relaxed));
                if !conns.contains(&id) {
                    break id;
                }
            };
            let conn = Connection(Arc::new(
                ConnImpl {
                    id, tx,
                    state: Mutex::new(ConnState {
                        subscriptions: HashMap::new(),
                    }),
                }
            ));
            conns.insert(conn.clone());
            conn
        };
        let disp = dispatcher::Dispatcher {
            conn: conn.clone(),
            graphql: graphql.clone(),
            incoming: self.clone(),
        };
        let fut = Loop::server(output, input, rx, disp,
            &*WEBSOCK_CONFIG, &handle());
        return (Token {
            key: conn.0.id,
            holder: self.clone(),
        }, fut);
     }
     pub fn subscribe(&self, conn: &Connection, subscription: Subscription,
        id: &String, input: &Input)
     {
        conn.0.state.lock().expect("lock is not poisoned")
            .subscriptions.entry(subscription.clone())
            .or_insert_with(HashMap::new)
            .insert(id.clone(), input.clone());
        self.0.state.lock().expect("lock is not poisoned")
            .subscriptions.entry(subscription)
            .or_insert_with(HashSet::new)
            .insert(conn.clone());
     }
     pub fn unsubscribe_id(&self, conn: &Connection, id: &String)
     {
        if let Some(subscription) = conn.unsubscribe_id(id) {
            let mut state = self.0.state.lock().expect("lock is not poisoned");
            let remove = state.subscriptions.get_mut(&subscription)
                .map(|x| {
                    x.remove(conn);
                    x.len() == 0
                }).unwrap_or(false);
            if remove {
                state.subscriptions.remove(&subscription);
            }
        }
     }
     pub fn trigger(&self, subscription: &Subscription) {
        let conns = self.0.state.lock().expect("lock is not poisoned")
            .subscriptions.get(&subscription)
            .map(|x| x.clone())
            .unwrap_or_else(HashSet::new);
        for conn in conns {
            let clock = conn.0.state.lock().expect("lock is not poisoned");
            let items = clock.subscriptions.get(&subscription);
            for (id, input) in items.iter().flat_map(|x| *x) {
                let result = graphql::ws_response(&self.0.context, &input);
                let packet = Packet::Text(
                    to_string(&OutputMessage::Data {
                        id: id.clone(),
                        payload: result,
                    })
                    .expect("can serialize"));
                conn.0.tx.unbounded_send(packet)
                    .map_err(|e| {
                        trace!("can't reply with ack: {}", e)
                    }).ok();
            }
        }
     }
}

impl Borrow<Id> for Connection {
    fn borrow(&self) -> &Id {
        &self.0.id
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        let mut state = self.holder.0.state.lock().expect("lock works");
        state.connections.remove(&self.key);
        state.subscriptions.retain(|_k, v| {
            v.remove(&self.key);
            return !v.is_empty();
        })
    }
}

impl PartialEq for Connection {
    fn eq(&self, other: &Connection) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for Connection {}

impl ::std::hash::Hash for Connection {
    fn hash<H: ::std::hash::Hasher>(&self, h: &mut H)  {
        self.0.id.hash(h)
    }
}

impl Connection {
    fn unsubscribe_id(&self, id: &String) -> Option<Subscription> {
        let mut state = self.0.state.lock().expect("lock is not poisoned");
        for (k, v) in &mut state.subscriptions {
            if v.contains_key(id) {
                v.remove(id);
                if v.len() == 0 {
                    return Some(k.clone());
                }
                break;
            }
        }
        return None;
    }
    pub fn received(&self, packet: Packet) {
        self.0.tx.unbounded_send(packet)
            .map_err(|e| trace!("can't send message to connection: {}", e))
            .ok();
    }
}

// TODO(tailhook) move somewhere
pub fn subscription_to_query(doc: Document) -> Document {
    let definitions = doc.definitions.into_iter().map(|def| {
        match def {
            Definition::Operation(Op::Subscription(s)) => {
                Definition::Operation(Op::Query(QueryParams {
                    position: s.position,
                    name: s.name,
                    variable_definitions: s.variable_definitions,
                    directives: s.directives,
                    selection_set: s.selection_set,
                }))
            }
            def => def,
        }
    }).collect();
    return Document { definitions }
}
