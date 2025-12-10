use anyhow::{bail, Result};
use rsip::message::HeadersExt;
use rsip::{Method, StatusCode};
use rsip::headers::ToTypedHeader;
use tracing::{debug, info};
use ucware_cli::cmd;

#[tokio::main]
async fn main() -> Result<()> {
    let (client, _args) = cmd::init::<()>().await?;

    let (_socket, mut requests) = client.socket().await?;

    loop {
        let Some(mut tx) = requests.recv().await else {
            bail!("Client closed connection");
        };

        debug!("Request: {request:#?}", request = tx.request);

        match tx.request.method {
            Method::Options => {
                tx.respond(StatusCode::Accepted).send([]).await;
            }

            Method::Invite => {
                let from = tx.request.from_header().expect("valid from header");
                let seq = tx.request.cseq_header().expect("cseq").seq().expect("cseq");

                let from = from.typed().expect("valid from header");

                info!("Invite: {seq}: {from:?}");

                tx.respond(StatusCode::Trying).send([]).await;
                tx.respond(StatusCode::Ringing).send([]).await;
            }

            Method::Cancel => {
                let seq = tx.request.cseq_header().expect("cseq").seq().expect("cseq");
                info!("Cancel: {seq}");

                tx.respond(StatusCode::Accepted).send([]).await;
            }

            _ => {}
        }
    }
}
