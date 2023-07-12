mod websocket;

use anyhow::Result;
use clia_ntex_files as fs;
use clia_ntex_identity::{CookieIdentityPolicy, Identity, IdentityService, RequestIdentity};
use clia_ntex_session::{CookieSession, Session};
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use ntex::http::header::ContentEncoding;
use ntex::web::{self, get, Error, ErrorRenderer, WebRequest, WebResponse};
use ntex::{Middleware, Service, ServiceCtx};
use rand::RngCore;

async fn index(session: Session, id: Identity) -> Result<String, Error> {
    // access session data
    let count = if let Some(count) = session.get::<i32>("counter")? {
        session.set("counter", count + 1)?;
        count
    } else {
        session.set("counter", 1)?;
        0
    };

    let res = if let Some(id) = id.identity() {
        format!("Welcome! {id} counter:{count}")
    } else {
        format!("Welcome! Anonymous! counter:{count}")
    };

    Ok(res)
}

async fn login(id: Identity) -> web::HttpResponse {
    id.remember("User1".to_owned()); // <- remember identity
    web::HttpResponse::Ok().body("login ok")
}

#[get("/logout")]
async fn logout(id: Identity) -> web::HttpResponse {
    id.forget(); // <- remove identity
    web::HttpResponse::Ok().body("logout ok")
}

#[ntex::main]
async fn main() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();
    let mut key = [0; 32];
    rand::thread_rng().fill_bytes(&mut key);
    web::HttpServer::new(move || {
        web::App::new()
            .wrap(web::middleware::Logger::default())
            .wrap(StaticFileIdentityCheckService)
            .wrap(web::middleware::Compress::new(ContentEncoding::Auto))
            .wrap(
                CookieSession::signed(&[1; 32]) // <- create cookie based session middleware
                    .name("session")
                    .path("/")
                    .secure(false)
                    .max_age(10),
            )
            .wrap(IdentityService::new(
                // <- create identity middleware
                CookieIdentityPolicy::new(&key) // <- create cookie identity policy
                    .name("auth-cookie")
                    .secure(false)
                    .visit_deadline(time::Duration::days(1)),
            ))
            .service(fs::Files::new("/static", ".").show_files_listing())
            .service(web::resource("/").to(index))
            .service(web::resource("/login").to(login))
            .service(web::resource("/wss/{server_id}").to(websocket::ws_index))
            .service(logout)
    })
    .bind(("0.0.0.0", 80))?
    .bind(("0.0.0.0", 8080))?
    .run()
    .await?;
    Ok(())
}

pub struct StaticFileIdentityCheckService;

impl<S> Middleware<S> for StaticFileIdentityCheckService {
    type Service = StaticFileIdentityCheck<S>;

    #[inline]
    fn create(&self, service: S) -> Self::Service {
        StaticFileIdentityCheck { service }
    }
}

pub struct StaticFileIdentityCheck<S> {
    service: S,
}

impl<S, E> Service<WebRequest<E>> for StaticFileIdentityCheck<S>
where
    S: Service<WebRequest<E>, Response = WebResponse, Error = Error> + 'static,
    E: ErrorRenderer,
{
    type Response = WebResponse;
    type Error = S::Error;
    type Future<'f> = LocalBoxFuture<'f, Result<Self::Response, Self::Error>>;

    ntex::forward_poll_ready!(service);

    #[inline]
    fn call<'a>(&'a self, req: web::WebRequest<E>, ctx: ServiceCtx<'a, Self>) -> Self::Future<'_> {
        Box::pin(async move {
            println!("{}", req.path());
            if req.path().starts_with("/static") {
                if let Some(id) = req.get_identity() {
                    log::debug!("{}", id);
                    ctx.call(&self.service, req).await
                } else {
                    Ok(req.into_response(
                        web::HttpResponse::TemporaryRedirect()
                            .set_header("Location", "/")
                            .finish(),
                    ))
                }
            } else {
                ctx.call(&self.service, req).await
            }
        })
        .boxed_local()
    }
}
