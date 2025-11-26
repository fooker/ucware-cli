use anyhow::{Context, Result};
use clap::Parser;
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

    let token = TokenStore::open(".token", || {
        let Some(token) = args.token else {
            anyhow::bail!("No stored token available and no token specified - get a token, kid!")
        };
        Ok(token)
    })
    .await?;

    let client = Client::new(url, token)?;
    client.refresh_token().await?;

    let slots = client.user().slots().get_all().await?;
    for slot in &slots {
        println!("Slot: {:#?}", slot);
    }

    let _slot = slots
        .into_iter()
        .find(|slot| slot.name == "UCC-Client")
        .context("No matching slot found")?;

    Ok(())
}
