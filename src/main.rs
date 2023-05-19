mod websocket;

use anyhow::Result;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use ntex::web::{self, get, Error, ErrorRenderer, WebRequest, WebResponse};
use ntex::{Middleware, Service};
use ntex_files as fs;
use ntex_identity::{CookieIdentityPolicy, Identity, IdentityService, RequestIdentity};
use ntex_session::{CookieSession, Session};

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
    web::HttpServer::new(|| {
        web::App::new()
            .wrap(web::middleware::Logger::default())
            .wrap(StaticFileIdentityCheckService)
            .wrap(
                CookieSession::signed(&[1; 32]) // <- create cookie based session middleware
                    .name("session")
                    .path("/")
                    .secure(false)
                    .max_age(10),
            )
            .wrap(IdentityService::new(
                // <- create identity middleware
                CookieIdentityPolicy::new(&[10; 32]) // <- create cookie identity policy
                    .name("auth-cookie")
                    .secure(false)
                    .visit_deadline(time::Duration::days(1)),
            ))
            .service(fs::Files::new("/static", ".").show_files_listing())
            .service(web::resource("/").to(index))
            .service(web::resource("/login").to(login))
            .service(web::resource("/ws").to(websocket::ws_index))
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
    fn call(&self, req: WebRequest<E>) -> Self::Future<'_> {
        Box::pin(async move {
            if req.path().starts_with("/static") {
                if let Some(id) = req.get_identity() {
                    log::debug!("{}", id);
                    self.service.call(req).await
                } else {
                    Ok(req.into_response(
                        web::HttpResponse::MovedPermanently()
                            .set_header("Location", "/")
                            .finish(),
                    ))
                }
            } else {
                self.service.call(req).await
            }
        })
        .boxed_local()
    }
}
