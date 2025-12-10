use anyhow::{bail, Result};
use dashmap::DashMap;
use notify_rust::{Hint, Notification, Timeout};
use rsip::headers::ToTypedHeader;
use rsip::message::HeadersExt;
use rsip::{Method, StatusCode};
use ucware_cli::cmd;

#[tokio::main]
async fn main() -> Result<()> {
    let (client, _args) = cmd::init::<()>().await?;

    let (_socket, mut requests) = client.socket().await?;

    let notifications = DashMap::new();

    loop {
        let Some(mut tx) = requests.recv().await else {
            bail!("Client closed connection");
        };

        match tx.request.method {
            Method::Options => {
                tx.respond(StatusCode::Accepted).send([]).await;
            }

            Method::Invite => {
                let from = tx.request.from_header().expect("valid from header");
                let cseq = tx.request.cseq_header().expect("valid cseq header");

                let from = from.typed().expect("valid from header");
                let cseq = cseq.typed().expect("valid cseq header");

                tx.respond(StatusCode::Trying).send([]).await;
                tx.respond(StatusCode::Ringing).send([]).await;

                let notification = Notification::new()
                    .summary("Incoming Call")
                    .body(&format!("{}", from.display_name.as_ref().map(String::as_str).unwrap_or("Unknown")))
                    .icon("phone")
                    .hint(Hint::Resident(true))
                    .timeout(Timeout::Never)
                    .show_async().await?;
                notifications.insert(cseq.seq, notification);
            }

            Method::Cancel => {
                let cseq = tx.request.cseq_header().expect("valid cseq header");
                let cseq = cseq.typed().expect("valid cseq header");

                tx.respond(StatusCode::Accepted).send([]).await;

                if let Some((_, notification)) = notifications.remove(&cseq.seq) {
                    notification.close();
                }
            }

            _ => {}
        }
    }
}
