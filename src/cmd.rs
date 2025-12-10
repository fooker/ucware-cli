use crate::ucware::{Client, TokenStore};
use anyhow::{anyhow, Result};
use clap::{Args, Parser};
use url::Url;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct CmdArgs<A>
where
    A: Args,
{
    #[command(flatten)]
    verbosity: clap_verbosity_flag::Verbosity,

    #[arg(short, long)]
    url: Url,

    #[arg(short, long)]
    token: Option<String>,

    #[clap(flatten)]
    inner: A,
}

// impl<A> Deref for CmdArgs<A>
// where
//     A: Args,
// {
//     type Target = A;
//
//     fn deref(&self) -> &Self::Target {
//         &self.args
//     }
// }

pub async fn init<A: Args>() -> Result<(Client, A)> {
    let args = CmdArgs::<A>::parse();

    tracing_subscriber::fmt()
        .with_max_level(args.verbosity)
        .init();

    let token = match args.token {
        None => TokenStore::open(".token")
            .await?
            .ok_or_else(|| anyhow!("No token specified and no store available")),
        Some(token) => TokenStore::with_token(".token", token).await,
    }?;

    let client = Client::new(args.url, token)?;
    client.refresh_token().await?;

    Ok((client, args.inner))
}
