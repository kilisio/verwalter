extern crate rand;
extern crate libc;
extern crate regex;
extern crate quire;
extern crate scan_dir;
extern crate argparse;
extern crate tempfile;
extern crate yaml_rust;
extern crate handlebars;
extern crate indexed_log;
extern crate rustc_serialize;
#[macro_use] extern crate quick_error;
#[macro_use] extern crate lazy_static;

#[macro_use] mod macros;
mod fs_util;
mod apply;
mod render;
mod config;

use std::path::{PathBuf};
use std::process::exit;

use argparse::{ArgumentParser, Parse, StoreTrue, FromCommandLine};
use indexed_log::Index;
use rustc_serialize::json::Json;

struct ParseJson(Json);

impl FromCommandLine for ParseJson {
    fn from_argument(s: &str) -> Result<ParseJson, String> {
        Json::from_str(s).map_err(|x| x.to_string()).map(ParseJson)
    }
}


fn main() {
    let mut vars = ParseJson(Json::Null);
    let mut log_dir = PathBuf::from("/var/log/verwalter");
    let mut template_dir = PathBuf::from("/etc/verwalter/templates");
    let mut dry_run = false;
    {
        let mut ap = ArgumentParser::new();
        ap.set_description("Internal verwalter's utility to render it");
        ap.refer(&mut vars).add_argument("vars", Parse, "
            Variables to pass to renderer
            ").required();
        ap.refer(&mut dry_run).add_option(&["--dry-run"], StoreTrue, "
            Don't run commands just show the templates and command-lines.
            ");
        ap.refer(&mut log_dir).add_option(&["--log-dir"], Parse, "
            Log directory (default `/var/log/verwalter`)");
        ap.refer(&mut template_dir).add_option(&["--template-dir"], Parse, "
            Directory with templates (default `/etc/verwalter/templates`");
        ap.parse_args_or_exit();
    }
    let vars = match vars {
        ParseJson(Json::Object(v)) => v,
        _ => exit(3),
    };
    let mut log = Index::new(&log_dir, dry_run);
    let id = match vars.get("deployment_id").and_then(|x| x.as_string()) {
        Some(x) => x.to_string(),
        None => exit(3),
    };
    let role = match vars.get("role").and_then(|x| x.as_string()) {
        Some(x) => x.to_string(),
        None => exit(3),
    };
    let template = match vars.get("template").and_then(|x| x.as_string()) {
        Some(x) => x.to_string(),
        None => exit(4),
    };
    match vars.get("verwalter_version").and_then(|x| x.as_string()) {
        Some(concat!("v", env!("CARGO_PKG_VERSION"))) => {},
        Some(_) => exit(5),
        None => exit(3),
    };

    let mut dlog = log.deployment(&id, false);
    {
        let mut rlog = match dlog.role(&role, false) {
            Ok(rlog) => rlog,
            Err(_) => exit(81),
        };
        match render::render_role(&template_dir.join(template),
                                  &Json::Object(vars), &mut rlog)
        {
            Err(e) => {
                rlog.log(format_args!(
                    "ERROR: Can't render templates: {}\n", e));
                // TODO(tailhook) should we still check dlog.errors()
                exit(10);
            }
            Ok(actions) => {
                match apply::apply_list(&role, actions, &mut rlog, dry_run) {
                    Err(e) => {
                        rlog.log(format_args!(
                            "ERROR: Can't apply templates: {}\n", e));
                        // TODO(tailhook) should we still check dlog.errors()
                        exit(20);
                    }
                    Ok(()) => {}
                }
            }
        }
    }
    if dlog.errors().len() != 0 {
        exit(81);
    }
}