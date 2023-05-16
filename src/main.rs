use anyhow::Result;
use ntex::web::{self,  Error, get};
use ntex_files as fs;
use ntex_session::{CookieSession, Session};
use ntex_identity::{Identity, CookieIdentityPolicy, IdentityService};
use time::Duration;


async fn index(session: Session,id: Identity) -> Result<String, Error> {
    // access session data
    let count= if let Some(count) = session.get::<i32>("counter")? {
        session.set("counter", count+1)?;
        count
    } else {
        session.set("counter", 1)?;
        0
    };

    let res= if let Some(id) = id.identity() {
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
    id.forget();                      // <- remove identity
    web::HttpResponse::Ok().body("logout ok")
}

#[ntex::main]
async fn main()->Result<()> {
    env_logger::builder().filter_level(log::LevelFilter::Debug).init();
    web::HttpServer::new(|| {
        web::App::new()
            .wrap(CookieSession::signed(&[1; 32]) // <- create cookie based session middleware
                .name("session")
                .path("/")
                .secure(false)
                .max_age(10)

            )
            .wrap(IdentityService::new(
                // <- create identity middleware
                CookieIdentityPolicy::new(&[10; 32])    // <- create cookie identity policy
                    .name("auth-cookie")
                    .secure(false)
                    .visit_deadline(Duration::days(1))))
            .service(fs::Files::new("/static", ".").show_files_listing())
            .service(web::resource("/").to(index))
            .service(web::resource("/login").to(login))
            .service(logout)
    })
    .bind(("0.0.0.0", 80))?
    .bind(("0.0.0.0", 8080))?
    .run()
    .await?;
    Ok(())
}
