use std::io::{self, Read, Write, Seek};
use std::str::from_utf8;
use std::path::Path;
use std::path::Component::Normal;
use std::fs::{File, metadata};
use std::time::{Duration};
use std::ascii::AsciiExt;
use std::collections::{HashMap, HashSet};

use futures::{Future, Async};
use gron::json_to_gron;
use rustc_serialize::Encodable;
use rustc_serialize::json::{as_json, as_pretty_json, Json};
use tk_http::server::{Codec as CodecTrait, Dispatcher as DispatcherTrait};
use tk_http::server::{Head, Encoder, EncoderDone, RecvMode, Error};

use id::Id;
use elect::Epoch;
use shared::{SharedState, PushActionError};
use time_util::ToMsec;

mod to_json;

use self::to_json::ToJson;

const MAX_LOG_RESPONSE: u64 = 1048576;

#[derive(Clone, Debug)]
pub enum ApiRoute {
    Status,
    Peers,
    Schedule,
    Scheduler,
    SchedulerInput,
    SchedulerDebugInfo,
    Election,
    PushAction,
    ActionIsPending(u64),
    PendingActions,
    ForceRenderAll,
}

#[derive(Clone, Copy, Debug)]
pub enum Range {
    FromTo(u64, u64),
    AllFrom(u64),
    Last(u64),
}


#[derive(Clone, Debug)]
pub enum LogRoute {
    Index(String, Range),
    Global(String, Range),
    Changes(String, Range),
    Role(String, Range),
    External(String, Range),
}

#[derive(Clone, Debug, Copy)]
pub enum Format {
    Json,
    Gron,
    Plain,
}

#[derive(Clone, Debug)]
pub enum Route {
    Index,
    Static(String),
    Api(ApiRoute, Format),
    Log(LogRoute),
    NotFound,
}

pub struct Dispatcher(pub SharedState);

pub struct Codec {
    state: SharedState,
    route: Route,
}


fn path_component(path: &str) -> (&str, &str) {
    let path = if path.starts_with('/') {
        &path[1..]
    } else {
        path
    };
    match path.bytes().position(|x| x == b'/') {
        Some(end) => (&path[..end], &path[end+1..]),
        None => {
            let end = path.bytes().position(|x| x == b'.')
                .unwrap_or(path.as_bytes().len());
            (&path[..end], "")
        }
    }
}

fn suffix(path: &str) -> &str {
    match path.bytes().rposition(|x| x == b'.' || x == b'/') {
        Some(i) if path.as_bytes()[i] == b'.' => &path[i+1..],
        Some(_) => "",
        None => "",
    }
}

/*
fn read_file<P:AsRef<Path>>(path: P, res: &mut Response)
    -> Result<(), io::Error>
{
    let path = path.as_ref();
    let mut file = try!(File::open(path));
    let mut buf = Vec::with_capacity(1024);
    try!(file.read_to_end(&mut buf));
    res.status(200, "OK");
    res.add_length(buf.len() as u64).unwrap();
    if path.extension().and_then(|x| x.to_str()) == Some("js") {
        res.add_header("Content-Type", b"text/javascript; charset=utf-8")
            .unwrap();
    }
    // TODO(tailhook) guess mime type
    res.done_headers().unwrap();
    res.write_body(&buf);
    res.done();
    Ok(())
}

fn serve_log(route: &LogRoute, ctx: &Context, res: &mut Response)
    -> io::Result<()>
{
    use self::LogRoute::*;
    use self::Range::*;
    let (path, rng) = match *route {
        Index(ref tail, rng) => {
            let path = ctx.log_dir.join(".index").join(tail);
            (path, rng)
        }
        Global(ref tail, rng) => {
            let path = ctx.log_dir.join(".global").join(tail);
            (path, rng)
        }
        Changes(ref tail, rng) => {
            let path = ctx.log_dir.join(".changes").join(tail);
            (path, rng)
        }
        Role(ref tail, rng) => {
            let path = ctx.log_dir.join(tail);
            (path, rng)
        }
        External(ref tail, rng) => {
            let (name, suffix) = path_component(tail);
            let path = match ctx.sandbox.log_dirs.get(name) {
                Some(path) => path.join(suffix),
                None => return Err(io::Error::new(io::ErrorKind::NotFound,
                    "directory not found in sandbox")),
            };
            (path, rng)
        }
    };
    let mut file = try!(File::open(&path));
    let meta = try!(metadata(&path));

    let (start, end) = match rng {
        FromTo(x, y) => (x, y + 1),
        AllFrom(x) => (x, meta.len()),
        Last(x) => (meta.len().saturating_sub(x), meta.len()),
    };
    let num_bytes = match end.checked_sub(start) {
        Some(n) => n,
        None => {
            return Err(io::Error::new(io::ErrorKind::InvalidData,
                "Request range is invalid"));
        }
    };

    if num_bytes > MAX_LOG_RESPONSE {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            "Requested range is too large"));
    }

    let mut buf = vec![0u8; num_bytes as usize];
    if start > 0 {
        try!(file.seek(io::SeekFrom::Start(start)));
    }
    try!(file.read(&mut buf));

    res.status(206, "OK");
    res.add_length(num_bytes).unwrap();
    res.add_header("Content-Range",
        format!("bytes {}-{}/{}", start, end-1, meta.len()).as_bytes()
    ).unwrap();
    res.done_headers().unwrap();
    res.write_body(&buf);
    res.done();
    Ok(())
}
*/


fn api_suffix(path: &str) -> Format {
    use self::Format::*;
    match suffix(path) {
        "pretty" => Plain,
        "gron" => Gron,
        _ => Json,
    }
}

fn parse_range_bytes(x: &str) -> Option<Range> {
    use self::Range::*;
    let mut iter = x.splitn(2, "-");
    match (iter.next(), iter.next()) {
        (_, None) | (None, _) => return None,
        (Some(""), Some(neg)) => neg.parse().ok().map(Last),
        (Some(st), Some("")) => st.parse().ok().map(AllFrom),
        (Some(start), Some(end)) => {
            start.parse().and_then(
                |start| end.parse().map(|end| FromTo(start, end))
            ).ok()
        }
    }
}

fn parse_range(head: &Head) -> Option<(&'static str, Range)> {
    let mut result = None;
    for (name, value) in head.headers() {
        if name.eq_ignore_ascii_case("Range") {
            let s = match from_utf8(value) {
                Ok(s) => s,
                // TODO(tailhook) implement 416 or 400
                Err(..) => return None,
            };
            if result.is_some() {
                // TODO(tailhook) implement 416 or 400
                return None;
            }
            if s.trim().starts_with("bytes=") {
                match parse_range_bytes(&s[6..]) {
                    Some(x) => result = Some(("bytes", x)),
                    // TODO(tailhook) implement 400
                    None => return None,
                }
            } else if s.trim().starts_with("records=") {
                match parse_range_bytes(&s[8..]) {
                    Some(x) => result = Some(("records", x)),
                    // TODO(tailhook) implement 400
                    None => return None,
                }
            } else {
                // TODO(tailhook) implement 400
                return None;
            }
        }
    }
    return result;
}

fn validate_path<P: AsRef<Path>>(path: P) -> bool {
    for cmp in Path::new(path.as_ref()).components(){
        match cmp {
            Normal(_) => {}
            _ => return false,
        }
    }
    return true;
}

fn parse_log_route(path: &str, head: &Head) -> Option<LogRoute> {
    use self::LogRoute::*;
    if !validate_path(path) {
        // TODO(tailhook) implement 400
        return None;
    }
    // TODO(tailhook) implement 416
    parse_range(head).and_then(|(typ, rng)| {
        match (path_component(path), typ) {
            (("index", tail), "bytes") => Some(Index(tail.into(), rng)),
            (("global", tail), "bytes") => Some(Global(tail.into(), rng)),
            (("changes", tail), "bytes") => Some(Changes(tail.into(), rng)),
            (("role", tail), "bytes") => Some(Role(tail.into(), rng)),
            (("external", tail), "bytes") => Some(External(tail.into(), rng)),
            _ => None,
        }
    })
}

fn parse_api(path: &str, head: &Head) -> Option<Route> {
    use self::Route::*;
    use self::ApiRoute::*;
    use self::Format::Plain;
    match path_component(path) {
        ("status", "") => Some(Api(Status, api_suffix(path))),
        ("peers", "") => Some(Api(Peers, api_suffix(path))),
        ("schedule", "") => Some(Api(Schedule, api_suffix(path))),
        ("scheduler", "") => Some(Api(Scheduler, api_suffix(path))),
        ("scheduler_input", "") => Some(Api(SchedulerInput, api_suffix(path))),
        ("scheduler_debug_info", "") => Some(Api(SchedulerDebugInfo, Plain)),
        ("election", "") => Some(Api(Election, api_suffix(path))),
        ("action", "") => Some(Api(PushAction, api_suffix(path))),
        ("force_render_all", "") => Some(Api(ForceRenderAll, Plain)),
        ("action_is_pending", tail) => {
            tail.parse().map(|x| {
                Api(ActionIsPending(x), api_suffix(path))
            }).ok()
        }
        ("pending_actions", "") => Some(Api(PendingActions, api_suffix(path))),
        ("log", tail) => parse_log_route(tail, &head).map(Log),
        _ => None,
    }
}

/*
fn respond<T: Encodable>(res: &mut Response, format: Format, data: T)
    -> Result<(), io::Error>
{
    res.status(200, "OK");
    let mut buf = Vec::with_capacity(8192);
    match format {
        Format::Json => {
            res.add_header("Content-Type", b"application/json").unwrap();
            try!(write!(&mut buf, "{}", as_json(&data)));
        }
        Format::Gron => {
            res.add_header("Content-Type", b"text/x-gron").unwrap();
            // TODO(tailhook) this should work without temporary conversions
            try!(write!(&mut buf, "{}", as_pretty_json(&data)));
            let tmpjson = Json::from_str(from_utf8(&buf).unwrap()).unwrap();
            buf.truncate(0);
            try!(json_to_gron(&mut buf, "json", &tmpjson));
        }
        Format::Plain => {
            res.add_header("Content-Type", b"application/json").unwrap();
            try!(write!(&mut buf, "{}", as_pretty_json(&data)));
        }
    };
    res.add_length(buf.len() as u64).unwrap();
    res.done_headers().unwrap();
    res.write_body(&buf);
    res.done();
    Ok(())
}

fn respond_text<T: AsRef<[u8]>>(res: &mut Response, data: T)
    -> Result<(), io::Error>
{
    let data = data.as_ref();
    res.status(200, "OK");
    res.add_header("Content-Type", b"text/plain").unwrap();
    res.add_length(data.len() as u64).unwrap();
    res.done_headers().unwrap();
    res.write_body(data);
    res.done();
    Ok(())
}

fn get_metrics() -> HashMap<&'static str, Json>
{
    unimplemented!();
    /*
    use scheduler::main as S;
    use elect::machine as M;
    use elect::network as N;
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

        ("broadcasts_sent", N::BROADCASTS_SENT.js()),
        ("broadcasts_errored", N::BROADCASTS_ERRORED.js()),
        ("pongs_sent", N::PONGS_SENT.js()),
        ("pongs_errored", N::PONGS_ERRORED.js()),
        ("last_ping_all", N::LAST_PING_ALL.js()),
        ("last_vote", N::LAST_VOTE.js()),
        ("last_confirm_vote", N::LAST_CONFIRM_VOTE.js()),
        ("last_pong", N::LAST_PONG.js()),
    ].into_iter().collect()
    */
}
*/

/*
fn serve_api(scope: &mut Scope<Context>, route: &ApiRoute,
    data: &[u8], format: Format, res: &mut Response)
    -> Result<(), io::Error>
{
    use self::ApiRoute::*;
    match *route {
        Status => {
            #[derive(RustcEncodable)]
            struct LeaderInfo<'a> {
                id: &'a Id,
                name: &'a str,
                hostname: &'a str,
                addr: Option<String>,
            }
            #[derive(RustcEncodable)]
            struct Status<'a> {
                version: &'static str,
                id: &'a Id,
                peers: usize,
                peers_timestamp: Option<u64>,
                leader: Option<LeaderInfo<'a>>,
                scheduler_state: &'static str,
                roles: usize,
                election_epoch: Epoch,
                last_stable_timestamp: u64,
                num_errors: usize,
                errors: &'a HashMap<&'static str, String>,
                failed_roles: &'a HashSet<String>,
                debug_force_leader: bool,
                self_report: Option<self_meter::Report>,
                threads_report: HashMap<String, self_meter::ThreadReport>,
                metrics: HashMap<&'static str, Json>,
            }
            let peers = scope.state.peers();
            let election = scope.state.election();
            let schedule = scope.state.schedule();
            let leader_id = if election.is_leader {
                Some(scope.state.id())
            } else {
                election.leader.as_ref()
            };
            let leader = leader_id.and_then(
                |id| peers.as_ref().and_then(|x| x.1.get(id)));
            let errors = scope.state.errors();
            let failed_roles = scope.state.failed_roles();
            let (me, thr) = {
                let meter = scope.meter.lock().unwrap();
                (meter.report(),
                 meter.thread_report()
                    .map(|x| x.map(|(k, v)| (k.to_string(), v)).collect())
                    .unwrap_or(HashMap::new()))
            };
            respond(res, format, &Status {
                version: concat!("v", env!("CARGO_PKG_VERSION")),
                id: scope.state.id(),
                peers: peers.as_ref().map(|x| x.1.len()).unwrap_or(0),
                peers_timestamp: peers.as_ref().map(|x| x.0.to_msec()),
                leader: leader.map(|peer| LeaderInfo {
                    id: leader_id.unwrap(),
                    name: &peer.name,
                    hostname: &peer.hostname,
                    addr: peer.addr.map(|x| x.to_string()),
                }),
                roles: schedule.map(|x| x.num_roles).unwrap_or(0),
                scheduler_state: scope.state.scheduler_state().describe(),
                election_epoch: election.epoch,
                last_stable_timestamp:
                    election.last_stable_timestamp.unwrap_or(0),
                num_errors: errors.len() + failed_roles.len(),
                errors: &*errors,
                failed_roles: &*failed_roles,
                debug_force_leader: scope.state.debug_force_leader(),
                self_report: me,
                threads_report: thr,
                metrics: get_metrics(),
            })
        }
        Peers => {
            respond(res, format, &scope.cantal.get_peers().as_ref()
                .map(|x| &x.peers))
        }
        Schedule => {
            if let Some(schedule) = scope.state.schedule() {
                respond(res, format, &schedule)
            } else {
                // TODO(tailhook) Should we return error code instead ?
                respond(res, format, Json::Null)
            }
        }
        Scheduler => {
            respond(res, format, &scope.state.scheduler_state())
        }
        SchedulerInput => {
            respond(res, format, &scope.state.scheduler_debug_info().0)
        }
        SchedulerDebugInfo => {
            respond_text(res, &scope.state.scheduler_debug_info().1)
        }
        Election => {
            respond(res, format, &scope.state.election())
        }
        PendingActions => {
            respond(res, format, &scope.state.pending_actions())
        }
        ForceRenderAll => {
            scope.state.force_render();
            respond(res, format, "ok")
        }
        PushAction => {
            let jdata = from_utf8(data).ok()
                .and_then(|x| Json::from_str(x).ok());
            match jdata {
                Some(x) => {
                    match scope.state.push_action(x) {
                        Ok(id) => {
                            respond(res, format, {
                                let mut h = HashMap::new();
                                h.insert("registered", id);
                                h
                            })
                        }
                        Err(PushActionError::TooManyRequests) => {
                            serve_error_page(429, res);
                            Ok(())
                        }
                        Err(PushActionError::NotALeader) => {
                            serve_error_page(421, res);
                            Ok(())
                        }
                    }
                }
                None => {
                    serve_error_page(400, res);
                    Ok(())
                }
            }
        }
        ActionIsPending(id) => {
            respond(res, format, {
                let mut h = HashMap::new();
                h.insert("pending", scope.state.check_action(id));
                h
            })
        }
    }
}


fn serve_error_page(code: u32, response: &mut Response) {
    let (status, reason) = match code {
        400 => (400, "Bad Request"),
        404 => (404, "Not Found"),
        408 => (408, "Request Timeout"),
        413 => (413, "Payload Too Large"),
        421 => (421, "Misdirected Request"),
        429 => (429, "Too Many Requests"),
        431 => (431, "Request Header Fields Too Large"),
        500 => (500, "Internal Server Error"),
        _ => unreachable!(),
    };
    response.status(status, reason);
    let data = format!("<h1>{} {}</h1>\n\
        <p><small>Served for you by rotor-http</small></p>\n",
        status, reason);
    let bytes = data.as_bytes();
    response.add_length(bytes.len() as u64).unwrap();
    response.add_header("Content-Type", b"text/html").unwrap();
    response.done_headers().unwrap();
    response.write_body(bytes);
    response.done();
}
*/

impl<S> CodecTrait<S> for Codec {
    type ResponseFuture = Box<Future<Item=EncoderDone<S>, Error=Error>>;

    fn recv_mode(&mut self) -> RecvMode {
        RecvMode::buffered_upfront(1_048_576)
    }

    fn start_response(&mut self, e: Encoder<S>) -> Self::ResponseFuture {
        unimplemented!();
    }

    fn data_received(&mut self,
                     data: &[u8],
                     end: bool)
                     -> Result<Async<usize>, Error>
    {
        assert!(end);
        unimplemented!();
        /*
        use self::Route::*;
        let iores = match *&self.0 {
            Index => read_file(scope.frontend_dir
                               .join("common/index.html"), res),
            Static(ref x) => {
                match read_file(scope.frontend_dir.join(&x), res) {
                    Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                        read_file(scope.frontend_dir
                            .join("common/index.html"), res)
                    }
                    res => res,
                }
            }
            Api(ref route, fmt) => serve_api(scope, route, data, fmt, res),
            Log(ref x) => {
                serve_log(x, scope, res)
            }
        };
        match iores {
            Ok(()) => {}
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => {
                serve_error_page(404, res);
            }
            Err(ref e) if e.kind() == io::ErrorKind::PermissionDenied => {
                serve_error_page(403, res);
            }
            Err(e) => {
                info!("Error serving {:?}: {}", self.0, e);
                serve_error_page(500, res);
            }
        }
        None
        */
    }

}

impl<S> DispatcherTrait<S> for Dispatcher {
    type Codec = Codec;
    fn headers_received(&mut self, headers: &Head)
        -> Result<Self::Codec, Error>
    {
        Ok(Codec {
            state: self.0.clone(),
            route: route(headers),
        })
    }
}


fn route(head: &Head) -> Route {
    use self::Route::*;
    let path = if let Some(path) = head.path() {
        path
    } else {
        return Route::NotFound;
    };
    let path = match path.find('?') {
        Some(x) => &path[..x],
        None => path,
    };
    let route = match path_component(&path[..]) {
        ("", _) => Some(Index),
        ("v1", suffix) => parse_api(suffix, &head),
        (_, _) => {
            if !validate_path(&path[1..]) {
                return Route::NotFound;
            }
            Some(Static(path[1..].to_string()))
        }
    };
    debug!("Routed {:?} to {:?}", path, route);
    route.unwrap_or(Route::NotFound)
}
