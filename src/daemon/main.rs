extern crate abstract_ns;
extern crate argparse;
extern crate cbor;
extern crate env_logger;
extern crate futures;
extern crate futures_cpupool;
extern crate gron;
extern crate handlebars;
extern crate inotify;
extern crate itertools;
extern crate libc;
extern crate libcantal;
extern crate nix;
extern crate ns_std_threaded;
extern crate quire;
extern crate rand;
extern crate regex;
extern crate rustc_serialize;
extern crate scan_dir;
extern crate self_meter_http;
extern crate sha1;
extern crate tempfile;
extern crate time;
extern crate tk_easyloop;
extern crate tk_http;
extern crate tk_listen;
extern crate tokio_core;
extern crate yaml_rust;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
#[macro_use] extern crate lua;
#[macro_use] extern crate matches;
#[macro_use] extern crate quick_error;

extern crate indexed_log;
extern crate verwalter_config as config;

use std::io::{stderr, Write};
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::process::exit;
use std::sync::mpsc::{channel, sync_channel};
use std::thread;

use abstract_ns::Resolver;
use futures::Future;
use time::now_utc;
use tk_easyloop::{run_forever, spawn, handle};
use tk_listen::ListenExt;

//use shared::{Id, SharedState};
use config::Sandbox;
use id::Id;

mod id;
mod info;
mod name;
mod http;
//mod shared;
/*
mod fs_util;
mod scheduler;
mod elect;
//mod frontend;
mod net;
mod info;
mod time_util;
//mod watchdog;
//mod fetch;
mod hash;
mod apply;
*/

use argparse::{ArgumentParser, Parse, ParseOption, StoreOption, StoreTrue};
use argparse::{Print};

pub struct Options {
    config_dir: PathBuf,
    storage_dir: PathBuf,
    log_dir: PathBuf,
    log_id: bool,
    dry_run: bool,
    hostname: Option<String>,
    name: Option<String>,
    listen_host: String,
    listen_port: u16,
    machine_id: Option<Id>,
    use_sudo: bool,
    debug_force_leader: bool,
}

fn init_logging(id: &Id, log_id: bool) {
    use std::env;
    use log::{LogLevelFilter, LogRecord};
    use env_logger::LogBuilder;

    let mut builder = LogBuilder::new();
    if log_id {
        let id = format!("{}", id);
        let format = move |record: &LogRecord| {
            let tm = now_utc();
            format!("{}.{:03}Z {} {}: {}",
                tm.strftime("%Y-%m-%dT%H:%M:%S").unwrap(),
                tm.tm_nsec / 1000000,
                id, record.level(), record.args())
        };
        builder.format(format).filter(None, LogLevelFilter::Warn);
    } else {
        let format = move |record: &LogRecord| {
            let tm = now_utc();
            format!("{}.{:03}Z {}:{}: {}",
                tm.strftime("%Y-%m-%dT%H:%M:%S").unwrap(),
                tm.tm_nsec / 1000000,
                record.level(), record.location().module_path(), record.args())
        };
        builder.format(format).filter(None, LogLevelFilter::Warn);
    }

    if let Ok(val) = env::var("RUST_LOG") {
       builder.parse(&val);
    }
    builder.init().unwrap();
}

fn main() {
    let mut options = Options {
        config_dir: PathBuf::from("/etc/verwalter"),
        storage_dir: PathBuf::from("/var/lib/verwalter"),
        log_dir: PathBuf::from("/var/log/verwalter"),
        log_id: false,
        dry_run: false,
        hostname: None,
        name: None,
        listen_host: "127.0.0.1".to_string(),
        listen_port: 8379,
        machine_id: None,
        use_sudo: false,
        debug_force_leader: false,
    };
    {
        let mut ap = ArgumentParser::new();
        ap.refer(&mut options.config_dir)
            .add_option(&["--config-dir"], Parse,
                "Directory of configuration files (default /etc/verwalter)");
        ap.refer(&mut options.storage_dir)
            .add_option(&["--storage-dir"], Parse,
                "Directory of configuration files \
                 (default /var/lib/verwalter)");
        ap.add_option(&["--version"],
            Print(env!("CARGO_PKG_VERSION").to_string()),
            "Show version and exit");
        ap.refer(&mut options.hostname)
            .add_option(&["--hostname"], ParseOption,
                "Hostname of current server");
        ap.refer(&mut options.name)
            .add_option(&["--name"], ParseOption,
                "Node name of current server (usually FQDN)");
        ap.refer(&mut options.machine_id)
            .add_option(&["--override-machine-id"], StoreOption,
                "Overrides machine id. Do not use in production, put the
                 file `/etc/machine-id` instead. This should only be used
                 for tests which run multiple nodes in single filesystem
                 image");
        ap.refer(&mut options.dry_run)
            .add_option(&["-n", "--dry-run"], StoreTrue, "
                Just try to render configs, and don't run anything real.
                Use with RUST_LOG=debug to find out every command that
                is about to run");
        ap.refer(&mut options.log_dir)
            .add_option(&["--log-dir"], Parse, "
                Directory for log files. The directory must be owned by
                verwalter, meaning that nobody should put any extra files
                there (because verwalter may traverse the directory)");
        ap.refer(&mut options.log_id)
            .add_option(&["--log-id"], StoreTrue, "
                Add own machine id to the log (Useful mostly for running
                containerized test in containers");
        ap.refer(&mut options.listen_host)
            .add_option(&["--host"], Parse, "
                Bind to host (ip), for web and cluster messaging");
        ap.refer(&mut options.listen_port)
            .add_option(&["--port"], Parse, "
                Bind to port, for web and cluster messaging");
        ap.refer(&mut options.use_sudo)
            .add_option(&["--use-sudo"], StoreTrue, "
                Run verwalter-render with sudo");
        ap.refer(&mut options.debug_force_leader)
            .add_option(&["--debug-force-leader"], StoreTrue, "
                Force this node to be a leader. This is useful for debugging
                purposes to force single process to be a leader without
                running as much processes as there are peers emulated by
                `fake-cantal.py` script. This means you can test large
                configs on single verwalter instance.");
        ap.parse_args_or_exit();
    }

    let id = match options.machine_id.clone().ok_or_else(|| info::machine_id())
    {
        Ok(id) => id,
        Err(Ok(id)) => id,
        Err(Err(e)) => {
            writeln!(&mut stderr(), "Error reading `/etc/machine-id`: {}. \
                The file is required.", e).ok();
            exit(3);
        }
    };
    let sandbox = match Sandbox::parse_all(options.config_dir.join("sandbox")){
        Ok(cfg) => cfg,
        Err(e) => {
            writeln!(&mut stderr(),
                "Error reading `/etc/sandbox`: {}", e).ok();
            exit(3);
        }
    };

    init_logging(&id, options.log_id);

    let meter = self_meter_http::Meter::new();
    meter.track_current_thread_by_name();

    run_forever(move || -> Result<(), Box<::std::error::Error>> {
        let ns = name::init(&meter);
        http::spawn_listener(&ns,
            &format!("{}:{}", options.listen_host, options.listen_port))?;
        Ok(())
    }).expect("loop starts");

/*
    let addr = (&options.listen_host[..], options.listen_port)
        .to_socket_addrs().expect("Can't resolve hostname")
        .collect::<Vec<_>>()[0];

    let schedule_file = options.storage_dir.join("schedule/schedule.json");
    debug!("Loading old schedule from {:?}", schedule_file);
    let old_schedule = match fs_util::read_json(&schedule_file)
        .map(scheduler::from_json)
    {
        Ok(Ok(x)) => {
            warn!("Started with a schedule from {} at {}",
                x.hash, x.timestamp);
            Some(x)
        }
        Ok(Err(e)) => {
            error!("Error decoding saved schedule: {}", e);
            None
        }
        Err(e) => {
            error!("Error reading schedule: {}", e);
            None
        }
    };

    let state = SharedState::new(id.clone(), options.debug_force_leader,
                                 old_schedule);
    let hostname = options.hostname
                   .unwrap_or_else(|| info::hostname().expect("gethostname"));
    // TODO(tailhook) resolve FQDN
    let name = options.name.unwrap_or_else(|| hostname.clone());

    let (alarm_tx, alarm_rx) = channel();

    let scheduler_settings = scheduler::Settings {
        id: id.clone(),
        hostname: hostname.clone(),
        config_dir: options.config_dir.clone(),
    };
    let scheduler_state = state.clone();
    let scheduler_alarm_tx = alarm_tx.clone();
    let mymeter = meter.clone();
    thread::spawn(move || {
        mymeter.lock().unwrap().track_current_thread("scheduler");
        let alarm = {
            let (tx, rx) = sync_channel(1);
            {scheduler_alarm_tx}.send(tx).expect("sent alarm task");
            rx.recv().expect("received alarm")
        };
        scheduler::run(scheduler_state, scheduler_settings, alarm)
    });

    let apply_settings = apply::Settings {
        dry_run: options.dry_run,
        use_sudo: options.use_sudo,
        hostname: hostname.clone(),
        log_dir: options.log_dir.clone(),
        config_dir: options.config_dir.clone(),
        schedule_file: schedule_file,
    };
    let apply_state = state.clone();
    let apply_alarm_tx = alarm_tx; // this is last one, no clone
    let mymeter = meter.clone();
    thread::spawn(move || {
        mymeter.lock().unwrap().track_current_thread("apply");
        let alarm = {
            let (tx, rx) = sync_channel(1);
            {apply_alarm_tx}.send(tx).expect("sent alarm task");
            rx.recv().expect("received alarm")
        };
        apply::run(apply_state, apply_settings, alarm);
    });

    info!("Started with machine id {}, listening {}", id, addr);
    net::main(&addr, id, hostname, name, state,
        options.config_dir.join("frontend"),
        &sandbox, options.log_dir.clone(),
        options.debug_force_leader,
        alarm_rx, meter)
        .expect("Error running main loop");
    unreachable!();
    */
}
