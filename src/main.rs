use anyhow::{bail, Context, Result};
use clap::Parser;
use rsip::message::HeadersExt;
use rsip::{Method, StatusCode};
use tracing::{debug, info};
use ucware_cli::sipsocket;
use ucware_cli::ucware::{Client, TokenStore};
use url::Url;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    #[arg(short, long)]
    url: Url,

    #[arg(short, long)]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_max_level(args.verbosity)
        .init();

    let url = args.url.join("api/2/").expect("valid URL");

    let token = TokenStore::open(".token", args.token).await?;

    let client = Client::new(url, token)?;
    client.refresh_token().await?;

    let slots = client.user().slots().get_all().await?;
    for slot in &slots {
        println!("Slot: {:#?}", slot);
    }

    let slot = slots
        .into_iter()
        .find(|slot| slot.name == "UCC-Client")
        .context("No matching slot found")?;

    let (mut c, mut requests) = sipsocket::Connection::connect(
        format!(
            "wss://{host}:{port}/sipsockets/",
            host = args.url.domain().expect("domain required"),
            port = slot.sip_port
        )
        .parse()
        .expect("valid URL"),
        &slot.sip_username,
    )
    .await?;

    c.register(&slot.sip_username, &slot.sip_password).await?;

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
                info!("Invite: {seq}: {from:?}");

                tx.respond(StatusCode::Trying)
                    .send([]).await;
                tx.respond(StatusCode::Ringing)
                    .send([]).await;
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
