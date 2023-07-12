use clia_ntex_identity::RequestIdentity;
use futures::channel::oneshot;
use futures::future::{select, Either};
use ntex::service::{fn_factory_with_config, fn_shutdown};
use ntex::time::sleep;
use ntex::util::Bytes;
use ntex::web::{types, ws, Error};
use ntex::{chain, fn_service, rt, web, Service};
use serde::Deserialize;
use serde_json::json;
use std::cell::RefCell;
use std::future::ready;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

// How often heartbeat pings are sent
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

struct WsState {
    /// Client must send ping at least once per 10 seconds (CLIENT_TIMEOUT),
    /// otherwise we drop connection.
    hb: Instant,
}


/// WebSockets service factory
async fn ws_service(
    sink: ws::WsSink,
    user_id: String,
    server_id: i32,
) -> Result<impl Service<ws::Frame, Response = Option<ws::Message>, Error = io::Error>, Error> {
    let state = Rc::new(RefCell::new(WsState { hb: Instant::now() }));

    // disconnect notification
    let (tx, rx) = oneshot::channel();

    // start heartbeat task
    rt::spawn(heartbeat(state.clone(), sink.clone(), rx));

    // handler service for incoming websockets frames
    let service = fn_service(move |frame| {
        let item = match frame {
            // update heartbeat
            ws::Frame::Ping(msg) => {
                state.borrow_mut().hb = Instant::now();
                Some(ws::Message::Pong(msg))
            }
            // update heartbeat
            ws::Frame::Pong(_) => {
                log::debug!("read user:{} pong", user_id);
                state.borrow_mut().hb = Instant::now();
                None
            }
            // send message back
            ws::Frame::Text(text) => Some(ws::Message::Text(
                String::from_utf8(Vec::from(text.as_ref())).unwrap().into(),
            )),
            ws::Frame::Binary(bin) => Some(ws::Message::Binary(bin)),
            // close connection
            ws::Frame::Close(reason) => Some(ws::Message::Close(reason)),
            // ignore other frames
            _ => None,
        };
        ready(Ok(item))
    });

    // handler service for shutdown notification that stop heartbeat task
    let on_shutdown = fn_shutdown(move || {
        let _ = tx.send(());
    });

    // pipe our service with on_shutdown callback
    Ok(chain(service).and_then(on_shutdown))
}

/// helper method that sends ping to client every heartbeat interval
async fn heartbeat(state: Rc<RefCell<WsState>>, sink: ws::WsSink, mut rx: oneshot::Receiver<()>) {
    loop {
        match select(Box::pin(sleep(HEARTBEAT_INTERVAL)), &mut rx).await {
            Either::Left(_) => {
                // check client heartbeats
                if Instant::now().duration_since(state.borrow().hb) > CLIENT_TIMEOUT {
                    // heartbeat timed out
                    println!("Websocket Client heartbeat failed, disconnecting!");
                    return;
                }

                // send ping
                if sink
                    .send(ws::Message::Ping(Bytes::default()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
            Either::Right(_) => {
                println!("Connection is dropped, stop heartbeat task");
                return;
            }
        }
    }
}

#[derive(Deserialize)]
pub struct WebSocketArgs {
    pub server_id: i32,
}

/// do websocket handshake and start web sockets service
pub async fn ws_index(
    req: web::HttpRequest,
    args: types::Path<WebSocketArgs>,
) -> Result<web::HttpResponse, Error> {
    let server_id = args.server_id;
    if let Some(user_id) = req.get_identity() {
        log::info!("user id:{} server id:{}", user_id, server_id);
        ws::start(
            req,
            fn_factory_with_config(move |sink| {
                let user_id = user_id.clone();
                async move { ws_service(sink, user_id, server_id).await }
            }),
        )
        .await
    } else {
        println!("not login");
        Ok(web::HttpResponse::BadRequest().json(&json!({"error":"session error"})))
    }
}
