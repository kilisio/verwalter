use std::path::{PathBuf, Path};
use std::time::{Duration, SystemTime, Instant};
use std::thread::sleep;
use std::process::exit;
use std::collections::{HashMap, BTreeMap};


use inotify::INotify;
use inotify::ffi::{IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE, IN_MOVED_FROM};
use inotify::ffi::{IN_MOVED_TO, IN_CREATE, IN_DELETE, IN_DELETE_SELF};
use inotify::ffi::{IN_MOVE_SELF};
use scan_dir::ScanDir;
use lua::GcOption;
//use rotor_cantal::{Dataset, Key, Value, Chunk};
use libcantal::{Counter, Integer};

use config;
use time_util::ToMsec;
use hash::hash;
//use watchdog::{Alarm, ExitOnReturn};
use shared::{Id, SharedState};
use scheduler::Schedule;
use scheduler::state::num_roles;
use rustc_serialize::json::{Json, ToJson};


lazy_static! {
    pub static ref SCHEDULING_TIME: Integer = Integer::new();
    pub static ref SCHEDULER_SUCCEEDED: Counter = Counter::new();
    pub static ref SCHEDULER_FAILED: Counter = Counter::new();
}


pub struct Settings {
    pub id: Id,
    pub hostname: String,
    pub config_dir: PathBuf,
}

fn watch_dir(notify: &mut INotify, path: &Path) {
    notify.add_watch(&path,
        IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE | IN_MOVED_FROM |
        IN_MOVED_TO | IN_CREATE | IN_DELETE | IN_DELETE_SELF |
        IN_MOVE_SELF)
    .map_err(|e| {
        warn!("Error adding directory {:?} to inotify: {}.",
              path, e);
    }).ok();
    ScanDir::dirs().walk(path, |iter| {
        for (entry, _) in iter {
            notify.add_watch(&entry.path(),
                IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE | IN_MOVED_FROM |
                IN_MOVED_TO | IN_CREATE | IN_DELETE | IN_DELETE_SELF |
                IN_MOVE_SELF)
            .map_err(|e| {
                warn!("Error adding directory {:?} to inotify: {}.",
                      entry.path(), e);
            }).ok();
        }
    }).map_err(|e| {
        warn!("Error when scanning config directory: {:?}", e);
    }).ok();
}
/*

fn convert_key(key: &Key) -> Json {
    use rotor_cantal::KeyVisitor::{Key, Value};
    let mut map = BTreeMap::new();
    let mut item = None;
    key.visit(|x| {
        match x {
            Key(k) => item = Some(k.to_string()),
            Value(v) => {
                map.insert(item.take().unwrap(), Json::String(v.into()));
            }
        }
    });
    return Json::Object(map);
}

fn convert_metrics(metrics: &HashMap<String, Dataset>) -> Json {
    Json::Object(
        metrics.iter()
        .map(|(name, metric)| (name.to_string(), convert_metric(metric)))
        .collect()
    )
}

fn convert_chunk(value: &Chunk) -> Json {
    use rotor_cantal::Chunk::*;
    match *value {
        Counter(ref vals) => vals.to_json(),
        Integer(ref vals) => vals.to_json(),
        Float(ref vals) => vals.to_json(),
        State(_) => unimplemented!(),
    }
}

fn convert_value(value: &Value) -> Json {
    use rotor_cantal::Value::*;
    match *value {
        Counter(x) => Json::U64(x),
        Integer(x) => Json::I64(x),
        Float(x) => Json::F64(x),
        State(_) => unimplemented!(),
    }
}

fn convert_metric(metric: &Dataset) -> Json {
    use rotor_cantal::Dataset::*;
    match *metric {
        SingleSeries(ref key, ref chunk, ref stamps) => {
            Json::Object(vec![
                ("type".into(), Json::String("single_series".into())),
                ("key".into(), convert_key(key)),
                ("values".into(), convert_chunk(chunk)),
                ("timestamps".into(), stamps.to_json()),
            ].into_iter().collect())
        },
        MultiSeries(ref items) => {
            Json::Object(vec![
                ("type".into(), Json::String("multi_series".into())),
                ("items".into(), Json::Array(items.iter()
                    .map(|&(ref key, ref chunk, ref stamps)| Json::Object(vec![
                        ("key".into(), convert_key(key)),
                        ("values".into(), convert_chunk(chunk)),
                        ("timestamps".into(), stamps.to_json()),
                        ].into_iter().collect()))
                    .collect())),
            ].into_iter().collect())
        },
        SingleTip(ref key, ref value, ref slc) => {
            Json::Object(vec![
                ("type".into(), Json::String("single_tip".into())),
                ("key".into(), convert_key(key)),
                ("value".into(), convert_value(value)),
                ("old_timestamp".into(), slc.0.to_json()),
                ("new_timestamp".into(), slc.1.to_json()),
            ].into_iter().collect())
        },
        MultiTip(ref items) => {
            Json::Object(vec![
                ("type".into(), Json::String("multi_tip".into())),
                ("items".into(), Json::Array(items.iter()
                    .map(|&(ref key, ref value, ref timestamp)|
                        Json::Object(vec![
                            ("key".into(), convert_key(key)),
                            ("value".into(), convert_value(value)),
                            ("timestamp".into(), timestamp.to_json()),
                            ].into_iter().collect()))
                    .collect())),
            ].into_iter().collect())
        }
        Chart(_) => unimplemented!(),
        Empty => Json::Null,
        Incompatible(_) => {
            Json::Object(vec![
                ("type".into(), Json::String("error".into())),
                ("error".into(), Json::String("incompatible".into())),
            ].into_iter().collect())
        }
    }
}
*/

pub fn main(state: SharedState, settings: Settings, mut alarm: Alarm) -> !
{
    let mut inotify = INotify::init().expect("create inotify");
    let _guard = ExitOnReturn(92);
    let mut scheduler = {
        let _alarm = alarm.after(Duration::from_secs(10));
        watch_dir(&mut inotify, &settings.config_dir.join("scheduler"));
        match super::read(settings.id.clone(),
                          settings.hostname.clone(),
                          &settings.config_dir)
        {
            Ok(s) => s,
            Err(e) => {
                error!("Scheduler load failed: {}", e);
                exit(4);
            }
        }
    };
    let mut runtime = {
        let _alarm = alarm.after(Duration::from_secs(2));
        watch_dir(&mut inotify, &settings.config_dir.join("runtime"));
        config::read_runtime(&settings.config_dir.join("runtime"))
    };
    loop {
        let mut cookie = state.wait_schedule_update(Duration::from_secs(5));

        while state.refresh_cookie(&mut cookie) {

            // TODO(tailhook) we reread everything on every iteration this
            // is waste of resources but for small configurations will be
            // negligible. Let's implement inotify later on
            let mut events = inotify.available_events()
                .expect("read inotify")
                .len();
            if events > 0 {
                debug!("Inotify events, waiting to become stable");
                {
                    let _alarm = alarm.after(Duration::from_secs(10));
                    while events > 0 {
                        // Since we rescan every file anyway, it's negligible
                        // to just rescan the whole directory tree for inotify
                        // too
                        watch_dir(&mut inotify, &settings.config_dir);
                        // Wait a little bit for filesystem to become stable.
                        // We intentinally add new directories first, so that
                        // we can track unstable changes in new directories
                        // too.
                        // 200 ms should be enough for file copy/backup tools,
                        //     but not for human interaction, which is fine.
                        sleep(Duration::from_millis(200));
                        events = inotify.available_events()
                                 .expect("read inotify")
                                 .len();
                    }
                }

                debug!("Directories stable. Reading configs");
                {
                    let _alarm = alarm.after(Duration::from_secs(10));
                    match super::read(settings.id.clone(),
                                      settings.hostname.clone(),
                                      &settings.config_dir)
                    {
                        Ok(s) => {
                            scheduler = s;
                            state.clear_error("scheduler_load");
                        }
                        Err(e) => {
                            state.set_error("scheduler_load", format!("{}", e));
                            error!("Scheduler load failed: {}. \
                                Using the old one.", e);
                        }
                    }
                }
                {
                    let _alarm = alarm.after(Duration::from_secs(2));
                    runtime = config::read_runtime(
                        &settings.config_dir.join("runtime"))
                }
            }

            let peers = state.peers().expect("peers are ready for scheduler");
            // TODO(tailhook) check if peers are outdated

            let timestamp = SystemTime::now();
            let instant = Instant::now();
            let _alarm = alarm.after(Duration::from_secs(1));

            let input = Json::Object(vec![
                ("now".to_string(), timestamp.to_msec().to_json()),
                ("current_host".to_string(), scheduler.hostname.to_json()),
                ("current_id".to_string(), scheduler.id.to_string().to_json()),
                ("parents".to_string(), Json::Array(
                    cookie.parent_schedules.iter()
                        .map(|s| s.data.clone()).collect()
                    )),
                ("actions".to_string(), Json::Object(
                    cookie.actions.iter()
                        .map(|(id, value)| (id.to_string(), value.to_json()))
                        .collect()
                    )),
                ("runtime".to_string(), runtime.data.to_json()),
                // TODO(tailhook) show runtime errors
                //("runtime_err".to_string(), runtime.errors.to_json()),
                ("peers".to_string(), Json::Object(
                    peers.1.iter()
                        .map(|(id, peer)| (id.to_string(), peer.to_json()))
                        .collect())),
                ("metrics".to_string(), Json::Object(Default::default()))
                /*
                    state.metrics()
                    .map(|x| Json::Object(x.items.iter()
                        .map(|(host, data)| (host.to_string(),
                            convert_metrics(data)))
                        .collect()))
                    .unwrap_or(Json::Null)),
                */
            ].into_iter().collect());


            let (result, dbg) = scheduler.execute(&input);
            SCHEDULING_TIME.set((Instant::now() - instant).to_msec() as i64);

            let json = match result {
                Ok(json) => {
                    state.clear_error("scheduler");
                    SCHEDULER_SUCCEEDED.incr(1);
                    json
                }
                Err(e) => {
                    error!("Scheduling failed: {}", e);
                    state.set_error("scheduler", format!("{}", e));
                    state.set_schedule_debug_info(input, dbg);
                    SCHEDULER_FAILED.incr(1);
                    sleep(Duration::from_secs(1));
                    continue;
                }
            };

            let hash = hash(json.to_string());
            state.set_schedule_by_leader(cookie, Schedule {
                num_roles: num_roles(&json),
                timestamp: timestamp.to_msec(),
                hash: hash,
                data: json,
                origin: scheduler.id.clone(),
            }, input, dbg);

            // We execute GC after every scheduler run, we are going to
            // sleep for quite a long time now, so don't care performance
            debug!("Garbage before collection: {}Kb, stack top: {}",
                scheduler.lua.gc(GcOption::Count, 0), scheduler.lua.get_top());
            scheduler.lua.gc(GcOption::Collect, 0);
            info!("Garbage after collection: {}Kb",
                scheduler.lua.gc(GcOption::Count, 0));
            break;
        }
    }
}
