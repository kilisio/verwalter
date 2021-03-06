use std::collections::{HashMap, HashSet};
use std::io::{Write, BufWriter};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, Duration};

use futures::future::{FutureResult, ok, Future};
use futures::future::{loop_fn, Loop::{Break, Continue}};
use futures::sync::oneshot;
use gron::json_to_gron;
use juniper;
use serde::Serialize;
use serde_json::{Value, to_writer, to_writer_pretty, to_value};
use serde_millis;
use tk_easyloop::timeout;
use tk_http::Status::{self, NotFound, PermanentRedirect};
use tk_http::Status::{TooManyRequests, ServiceUnavailable, InternalServerError};
use tk_http::server::{Codec as CodecTrait};
use tk_http::server::{Encoder, EncoderDone, Error};

use elect::{ElectionState};
use fetch;
use frontend::error_page::{error_page};
use frontend::routing::{ApiRoute, Format};
use frontend::to_json::ToJson;
use frontend::{Config, reply, read_json};
use id::Id;
use query::QueryData;
use shared::{SharedState, PushActionError};


pub type Request<S> = Box<CodecTrait<S, ResponseFuture=Reply<S>>>;
pub type Reply<S> = Box<Future<Item=EncoderDone<S>, Error=Error>>;

lazy_static! {
    static ref GRAPHIQL: String = juniper::http::graphiql::graphiql_source(
                    "/v1/graphql");
}


fn get_metrics() -> HashMap<&'static str, Value>
{
    use scheduler::main as S;
    use elect::machine as M;
    //use elect::network as N;
    vec![
        ("scheduling_time", S::SCHEDULING_TIME.js()),
        ("scheduler_succeeded", S::SCHEDULER_SUCCEEDED.js()),
        ("scheduler_failed", S::SCHEDULER_FAILED.js()),

        ("start_election_no", M::START_ELECTION_NO.js()),
        ("start_election_tm", M::START_ELECTION_TM.js()),
        ("ping_all_no", M::PING_ALL_NO.js()),
        ("ping_all_tm", M::PING_ALL_TM.js()),
        ("outdated_no", M::OUTDATED_NO.js()),
        ("outdated_tm", M::OUTDATED_TM.js()),
        ("ping_no", M::PING_NO.js()),
        ("ping_tm", M::PING_TM.js()),
        ("pong_no", M::PONG_NO.js()),
        ("pong_tm", M::PONG_TM.js()),
        ("vote_confirm_no", M::VOTE_CONFIRM_NO.js()),
        ("vote_confirm_tm", M::VOTE_CONFIRM_TM.js()),
        ("became_leader_no", M::BECAME_LEADER_NO.js()),
        ("became_leader_tm", M::BECAME_LEADER_TM.js()),
        ("vote_for_me_no", M::VOTE_FOR_ME_NO.js()),
        ("vote_for_me_tm", M::VOTE_FOR_ME_TM.js()),
        ("vote_other_no", M::VOTE_OTHER_NO.js()),
        ("vote_other_tm", M::VOTE_OTHER_TM.js()),
        ("late_vote_no", M::LATE_VOTE_NO.js()),
        ("late_vote_tm", M::LATE_VOTE_TM.js()),
        ("newer_ping_no", M::NEWER_PING_NO.js()),
        ("newer_ping_tm", M::NEWER_PING_TM.js()),
        ("new_vote_no", M::NEW_VOTE_NO.js()),
        ("new_vote_tm", M::NEW_VOTE_TM.js()),
        ("bad_hosts_no", M::BAD_HOSTS_NO.js()),
        ("bad_hosts_tm", M::BAD_HOSTS_TM.js()),
        ("self_elect_no", M::SELF_ELECT_NO.js()),
        ("self_elect_tm", M::SELF_ELECT_TM.js()),

        ("elect_start_no", M::ELECT_START_NO.js()),
        ("elect_start_tm", M::ELECT_START_TM.js()),
        ("elect_timeo_no", M::ELECT_TIMEO_NO.js()),
        ("elect_timeo_tm", M::ELECT_TIMEO_TM.js()),
        ("elect_voted_no", M::ELECT_VOTED_NO.js()),
        ("elect_voted_tm", M::ELECT_VOTED_TM.js()),
        ("elect_unresponsive_no", M::ELECT_UNRESPONSIVE_NO.js()),
        ("elect_unresponsive_tm", M::ELECT_UNRESPONSIVE_TM.js()),
        ("elect_conflict_no", M::ELECT_CONFLICT_NO.js()),
        ("elect_conflict_tm", M::ELECT_CONFLICT_TM.js()),
        ("elect_unsolicit_pong_no", M::ELECT_UNSOLICIT_PONG_NO.js()),
        ("elect_unsolicit_pong_tm", M::ELECT_UNSOLICIT_PONG_TM.js()),
        ("elect_newer_pong_no", M::ELECT_NEWER_PONG_NO.js()),
        ("elect_newer_pong_tm", M::ELECT_NEWER_PONG_TM.js()),

        //("broadcasts_sent", N::BROADCASTS_SENT.js()),
        //("broadcasts_errored", N::BROADCASTS_ERRORED.js()),
        //("pongs_sent", N::PONGS_SENT.js()),
        //("pongs_errored", N::PONGS_ERRORED.js()),
        //("last_ping_all", N::LAST_PING_ALL.js()),
        //("last_vote", N::LAST_VOTE.js()),
        //("last_confirm_vote", N::LAST_CONFIRM_VOTE.js()),
        //("last_pong", N::LAST_PONG.js()),
    ].into_iter().collect()
}

pub fn respond<D: Serialize, S>(mut e: Encoder<S>, format: Format, data: D)
    -> FutureResult<EncoderDone<S>, Error>
{
    e.status(Status::Ok);
    e.add_chunked().unwrap();
    let ctype = match format {
        Format::Json => "application/json",
        Format::Gron => "text/x-gron",
        Format::Plain => "application/json",
    };
    e.add_header("Content-Type", ctype.as_bytes()).unwrap();
    if e.done_headers().unwrap() {
        match format {
            Format::Json => {
                to_writer(&mut BufWriter::new(&mut e), &data)
                    .expect("data is always serializable");
            }
            Format::Gron => {
                json_to_gron(&mut BufWriter::new(&mut e), "json",
                    &to_value(data).expect("data is always convertible"))
                    .expect("data is always serializable");
            }
            Format::Plain => {
                to_writer_pretty(&mut BufWriter::new(&mut e), &data)
                    .expect("data is always serializable");
            }
        };
    }
    ok(e.done())
}

pub fn respond_cached_json<S>(mut e: Encoder<S>, data: impl AsRef<[u8]>)
    -> FutureResult<EncoderDone<S>, Error>
{
    e.status(Status::Ok);
    e.add_length(data.as_ref().len() as u64).unwrap();
    e.add_header("Content-Type", "application/json").unwrap();
    if e.done_headers().unwrap() {
        e.write_body(data.as_ref());
    }
    ok(e.done())
}

pub fn respond_text<S>(mut e: Encoder<S>, data: &str)
    -> FutureResult<EncoderDone<S>, Error>
{
    e.status(Status::Ok);
    e.add_chunked().unwrap();
    e.add_header("Content-Type",
                 "text/plain; charset=utf-8".as_bytes()).unwrap();
    if e.done_headers().unwrap() {
        e.write_body(data.as_bytes());
    }
    ok(e.done())
}

pub fn respond_html<S>(mut e: Encoder<S>, data: &str)
    -> FutureResult<EncoderDone<S>, Error>
{
    e.status(Status::Ok);
    e.add_chunked().unwrap();
    e.add_header("Content-Type",
                 "text/html; charset=utf-8".as_bytes()).unwrap();
    if e.done_headers().unwrap() {
        e.write_body(data.as_bytes());
    }
    ok(e.done())
}

pub fn respond_204<S>(mut e: Encoder<S>)
    -> FutureResult<EncoderDone<S>, Error>
{
    e.status(Status::NoContent);
    e.done_headers().unwrap();
    ok(e.done())
}

pub fn serve<S: 'static>(state: &SharedState, config: &Arc<Config>,
    route: &ApiRoute, format: Format, source: SocketAddr)
    -> Result<Request<S>, Error>
{
    use self::ApiRoute::*;
    let state = state.clone();
    let config = config.clone();
    match *route {
        Status => {
            Ok(reply(move |e| {
                #[derive(Serialize)]
                struct LeaderInfo<'a> {
                    id: &'a Id,
                    name: String,
                    hostname: String,
                    addr: Option<String>,
                    schedule: Option<&'a String>,
                }
                #[derive(Serialize)]
                struct Status<'a> {
                    version: &'static str,
                    id: &'a Id,
                    name: &'a str,
                    hostname: &'a str,
                    peers: usize,
                    #[serde(with="serde_millis")]
                    peers_timestamp: Option<SystemTime>,
                    leader: Option<LeaderInfo<'a>>,
                    roles: usize,
                    #[serde(with="serde_millis")]
                    last_stable_timestamp: Option<SystemTime>,
                    num_errors: usize,
                    errors: &'a HashMap<&'static str, String>,
                    failed_roles: &'a HashSet<String>,
                    debug_force_leader: bool,
                    //self_report: Option<self_meter::Report>,
                    //threads_report: HashMap<String, self_meter::ThreadReport>,
                    metrics: HashMap<&'static str, Value>,
                    fetch_state: Arc<fetch::PublicState>,
                    election_state: &'a Arc<ElectionState>,
                    schedule_id: Option<&'a String>,
                    schedule_status: &'a str,
                    default_frontend: &'a str,
                }
                let peers = state.peers();
                let election = state.election();
                let stable_schedule = state.stable_schedule();
                let owned_schedule;
                let leader_peer;
                let status;
                let leader = if election.is_leader {
                    owned_schedule = state.owned_schedule();
                    status = "ok";
                    Some(LeaderInfo {
                        id: state.id(),
                        name: state.name.clone(),
                        hostname: state.hostname.clone(),
                        // TODO(tailhook) resolve listening address and show
                        addr: None,
                        schedule: owned_schedule.as_ref().map(|x| &x.hash),
                    })
                } else {
                    match election.leader.as_ref()
                        .and_then(|id| peers.peers.get(id).map(|p| (id, p)))
                    {
                        Some((id, peer)) => {
                            leader_peer = peer.get();
                            let schedule_hash = leader_peer.schedule.as_ref()
                                                .map(|x| &x.hash);
                            status = match (schedule_hash, &stable_schedule) {
                                (Some(h), &Some(ref s)) if h == &s.hash => "ok",
                                (Some(_), _) => "fetching",
                                (None, _) => "waiting",
                            };
                            Some(LeaderInfo {
                                id: id,
                                name: leader_peer.name.clone(),
                                hostname: leader_peer.hostname.clone(),
                                addr: leader_peer.addr
                                    .map(|x| x.to_string()),
                                schedule: schedule_hash,
                            })
                        }
                        None => {
                            status = "unstable";
                            None
                        }
                    }
                };
                let errors = state.errors();
                let failed_roles = state.failed_roles();
                //let (me, thr) = {
                //    let meter = meter.lock().unwrap();
                //    (meter.report(),
                //     meter.thread_report()
                //        .map(|x| x.map(|(k, v)| (k.to_string(), v)).collect())
                //        .unwrap_or(HashMap::new()))
                //};
                Box::new(respond(e, format, &Status {
                    version: concat!("v", env!("CARGO_PKG_VERSION")),
                    id: &state.id,
                    name: &state.name,
                    hostname: &state.hostname,
                    peers: peers.peers.len(),
                    peers_timestamp: Some(peers.timestamp),
                    leader: leader,
                    roles: state.num_roles(),
                    last_stable_timestamp: election.last_stable_timestamp,
                    num_errors: errors.len() + failed_roles.len(),
                    errors: &*errors,
                    failed_roles: &*failed_roles,
                    debug_force_leader: state.debug_force_leader(),
                    //self_report: me,
                    //threads_report: thr,
                    metrics: get_metrics(),
                    fetch_state: state.fetch_state.get(),
                    election_state: &election,
                    schedule_id: stable_schedule.as_ref().map(|x| &x.hash),
                    schedule_status: status,
                    default_frontend: &config.default_frontend,
                }))
            }))
        }
        Peers => {
            #[derive(Serialize)]
            struct Peer<'a> {
                id: &'a Id,
                primary_addr: Option<String>,
                name: String,
                hostname: String,
                #[serde(with="::serde_millis")]
                known_since: SystemTime,
                #[serde(with="::serde_millis")]
                last_report_direct: Option<SystemTime>,
                errors: usize,
            }
            Ok(reply(move |e| {
                Box::new(respond(e, format,
                    &state.peers().peers.iter().map(|(id, peer)| {
                        let peer = peer.get();
                        Peer {
                            id: id,
                            name: peer.name.clone(),
                            hostname: peer.hostname.clone(),
                            primary_addr: peer.addr.map(|x| x.to_string()),
                            known_since: peer.known_since,
                            last_report_direct: peer.last_report_direct,
                            errors: peer.errors,
                        }
                    }).collect::<Vec<_>>()
                ))
            }))
        }
        Schedule => {
            match format {
                Format::Json => {
                    if let Some(text) = state.serialized_schedule() {
                        Ok(reply(move |e| {
                            Box::new(respond_cached_json(e, text.as_bytes()))
                        }))
                    } else {
                        Ok(reply(move |e| {
                            Box::new(error_page(NotFound, e))
                        }))
                    }
                }
                _ => {
                    if let Some(sched) = state.schedule() {
                        Ok(reply(move |e| {
                            Box::new(respond(e, format, sched))
                        }))
                    } else {
                        Ok(reply(move |e| {
                            Box::new(error_page(NotFound, e))
                        }))
                    }
                }
            }
        }
        SchedulerInput => {
            Ok(reply(move |e| {
                let info = state.scheduler_debug_info();
                if let Some(ref info) = *info {
                    Box::new(respond(e, format, &info.0))
                } else {
                    Box::new(error_page(NotFound, e))
                }
            }))
        }
        SchedulerDebugInfo => {
            Ok(reply(move |e| {
                let info = state.scheduler_debug_info();
                if let Some(ref info) = *info {
                    Box::new(respond_text(e, &info.1[..]))
                } else {
                    Box::new(error_page(NotFound, e))
                }
            }))
        }
        Election => {
            Ok(reply(move |e| {
                Box::new(respond(e, format, &*state.election()))
            }))
        }
        PendingActions => {
            Ok(reply(move |e| {
                Box::new(respond(e, format, &state.pending_actions()))
            }))
        }
        ForceRenderAll => {
            state.force_render();
            Ok(reply(move |e| {
                Box::new(respond(e, format, "ok"))
            }))
        }
        PushAction => {
            Ok(read_json(move |input: Value, e| {
                actions::log(source, &input);
                let (tx, _) = oneshot::channel();
                match state.push_action(input, tx) {
                    Ok(id) => {
                        #[derive(Serialize)]
                        struct Registered {
                            registered: u64,
                        }
                        Box::new(respond(e, format, &Registered {
                            registered: id,
                        }))

                    }
                    Err(PushActionError::TooManyRequests) => {
                        Box::new(error_page(TooManyRequests, e))
                    }
                    Err(PushActionError::NotALeader) => {
                        // Fix again to 'misdirected request' ?
                        Box::new(error_page(ServiceUnavailable, e))
                    }
                }
            }))
        }
        WaitAction => {
            Ok(read_json(move |input: Value, e| {
                actions::log(source, &input);
                let (tx, rx) = oneshot::channel();
                match state.push_action(input, tx) {
                    Ok(_id) => {
                        use shared::ActionError::*;
                        Box::new(
                            rx.then(move |res| match res {
                                Ok(Ok(val)) => {
                                    respond(e, format, &val)
                                }
                                // TODO(tailhook) show some error to user
                                Ok(Err(NoResponse)) => {
                                    respond_204(e)
                                }
                                Err(_) => {
                                    error_page(ServiceUnavailable, e)
                                }
                            }))

                    }
                    Err(PushActionError::TooManyRequests) => {
                        Box::new(error_page(TooManyRequests, e))
                    }
                    Err(PushActionError::NotALeader) => {
                        // Fix again to 'misdirected request' ?
                        Box::new(error_page(ServiceUnavailable, e))
                    }
                }
            }))
        }
        ActionIsPending(id) => {
            Ok(reply(move |e| {
                #[derive(Serialize)]
                struct Registered {
                    pending: bool,
                }
                Box::new(respond(e, format, &Registered {
                    pending: state.check_action(id),
                }))
            }))
        }
        Graphiql => {
            Ok(reply(move |e| {
                Box::new(respond_html(e, &*GRAPHIQL))
            }))
        }
        Backup(..) | Backups | Graphql | GraphqlWs(..) => unreachable!(),
        RedirectByNodeName => {
            Ok(reply(move |mut e| {
                Box::new(loop_fn(0, move |iter| {
                    let state = state.clone();
                    timeout(Duration::new(if iter > 0 { 1 } else { 0 }, 0))
                    .map_err(|_| unreachable!())
                    .and_then(move |()| {
                        let peers = state.peers();
                        let election = state.election();
                        if election.is_leader {
                            Ok(Break(Some(state.name.clone())))
                        } else {
                            match election.leader.as_ref()
                                .and_then(|id| peers.peers.get(id))
                            {
                                Some(peer) => {
                                    Ok(Break(Some(peer.get().name.clone())))
                                }
                                None => {
                                    if iter > 65 {
                                        Ok(Break(None))
                                    } else {
                                        Ok(Continue(iter+1))
                                    }
                                }
                            }
                        }
                    })
                })
                .and_then(move |hostname| {
                    if let Some(hostname) = hostname {
                        e.status(PermanentRedirect);
                        e.add_length(0).unwrap();
                        e.add_header("Cache-Control", "no-cache").unwrap();
                        // TODO(tailhook) fix port
                        // TODO(tailhook) append tail url
                        e.format_header("Location",
                            format_args!("http://{}:8379/", hostname))
                            .unwrap();
                        e.done_headers().unwrap();
                        ok(e.done())
                    } else {
                        e.status(NotFound);
                        e.add_chunked().unwrap();
                        e.add_header("Cache-Control", "no-cache").unwrap();
                        e.add_header("Content-Type", "text/plain").unwrap();
                        if e.done_headers().unwrap() {
                            write!(e, "No leader found in 65 seconds")
                                .unwrap();
                        }
                        ok(e.done())
                    }
                }))
            }))
        }
        RolesData => {
            Ok(reply(move |e| {
                Box::new(state.get_responder().get_roles_data()
                    .then(move |res| match res {
                        Ok(Ok(v)) => {
                            respond(e, format, v)
                        }
                        Ok(Err(err)) => {
                            error!("Error receiving roles_data: {}", err);
                            error_page(InternalServerError, e)
                        }
                        Err(err) => {
                            error!("Error receiving roles_data: {}", err);
                            error_page(ServiceUnavailable, e)
                        }
                    }))
            }))
        }
        Query(ref query) => {
            let query = query.clone();
            Ok(read_json(move |input: Value, e| {
                Box::new(state.get_responder().query(QueryData {
                        path: query.path,
                        body: input,
                    }).then(move |res| match res {
                        Ok(Ok(v)) => {
                            respond(e, format, v)
                        }
                        Ok(Err(err)) => {
                            error!("Error in query: {}", err);
                            error_page(InternalServerError, e)
                        }
                        Err(err) => {
                            error!("Error in query: {}", err);
                            error_page(ServiceUnavailable, e)
                        }
                    }))
            }))
        }
    }
}

mod actions {
    use std::net::SocketAddr;
    use serde_json::Value as Json;

    pub fn log(source: SocketAddr, input: &Json) {
        info!("Received action from {}: {}", source, input);
    }
}
