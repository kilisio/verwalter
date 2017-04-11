use std::time::Duration;
use std::process::exit;

use abstract_ns::{self, Resolver};
use futures::{Future, Stream};
use futures::future::{FutureResult, ok};
use tk_easyloop::{handle, spawn};
use tk_http;
use tk_http::{Status};
use tk_http::server::buffered::{Request, BufferedDispatcher};
use tk_http::server::{self, Encoder, EncoderDone, Proto, Error};
use tokio_core::net::TcpListener;
use tokio_core::io::Io;
use tk_listen::ListenExt;


fn service<S:Io>(_: Request, mut e: Encoder<S>)
    -> FutureResult<EncoderDone<S>, Error>
{
    const BODY: &'static str = "Hello World!";
    println!("SERVING");

    e.status(Status::Ok);
    e.add_length(BODY.as_bytes().len() as u64).unwrap();
    e.add_header("Server", "verwalter").unwrap();
    if e.done_headers().unwrap() {
        e.write_body(BODY.as_bytes());
    }
    ok(e.done())
}

pub fn spawn_listener(ns: &abstract_ns::Router, addr: &str)
    -> Result<(), Box<::std::error::Error>>
{
    let str_addr = addr.to_string();
    let hcfg = tk_http::server::Config::new()
        .inflight_request_limit(2)
        .inflight_request_prealoc(0)
        .first_byte_timeout(Duration::new(10, 0))
        .keep_alive_timeout(Duration::new(600, 0))
        .headers_timeout(Duration::new(1, 0))             // no big headers
        .input_body_byte_timeout(Duration::new(1, 0))     // no big bodies
        .input_body_whole_timeout(Duration::new(2, 0))
        .output_body_byte_timeout(Duration::new(1, 0))
        .output_body_whole_timeout(Duration::new(10, 0))  // max 65k bytes
        .done();

    spawn(ns.resolve(addr).map(move |addresses| {
        for addr in addresses.at(0).addresses() {
            info!("Listening on {}", addr);
            let listener = TcpListener::bind(&addr, &handle())
                .unwrap_or_else(|e| {
                    error!("Can't bind {}: {}", addr, e);
                    exit(81);
                });
            let hcfg = hcfg.clone();
            spawn(listener.incoming()
                .sleep_on_error(Duration::from_millis(100), &handle())
                .map(move |(socket, saddr)| {
                    println!("accepted {:?}", saddr);
                    Proto::new(socket, &hcfg,
                       BufferedDispatcher::new(saddr, &handle(), || service),
                       &handle())
                    .map_err(|e| debug!("Http protocol error: {}", e))
                })
                .listen(500)
                .then(move |res| {
                    error!("Listener {} exited: {:?}", addr, res);
                    exit(81);
                    Ok(())
                }));
        }
    }).map_err(move |e| {
        error!("Can't bind address {}: {}", str_addr, e);
        exit(3);
    }));
    Ok(())
}
