use anyhow::{anyhow, bail, Result};
use dashmap::DashMap;
use futures::{Sink, SinkExt, Stream, StreamExt, TryStreamExt};
use rand::distr::{Alphanumeric, SampleString};
use rsip::headers::auth::Algorithm;
use rsip::headers::{auth, CallId, ToTypedHeader, UntypedHeader};
use rsip::message::HeadersExt;
use rsip::services::DigestGenerator;
use rsip::{
    Auth, Header, Headers, Host, HostWithPort, Method, Param, Request, Response, Scheme,
    SipMessage, StatusCode, StatusCodeKind, Transport, Uri, Version,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use tokio::select;
use tokio::sync::mpsc;
use tracing::{info, trace, warn};
use url::Url;

use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
struct TransactionKey {
    method: String,
    call_id: Option<String>,
    branch: Option<String>,
}

impl TransactionKey {
    pub fn from_request(request: &Request) -> Self {
        let method = request.method.to_string();

        let call_id = request
            .call_id_header()
            .ok()
            .map(|header| header.to_string());

        let branch = request
            .via_header()
            .and_then(|header| header.typed())
            .ok()
            .and_then(|header| header.branch().map(ToString::to_string));

        Self {
            method,
            call_id,
            branch,
        }
    }

    pub fn from_response(response: &Response) -> Self {
        let method = response
            .cseq_header()
            .and_then(|header| header.typed())
            .ok()
            .map(|header| header.method.to_string())
            .unwrap_or_default();

        let call_id = response
            .call_id_header()
            .ok()
            .map(|header| header.to_string());

        let branch = response
            .via_header()
            .and_then(|header| header.typed())
            .ok()
            .and_then(|header| header.branch().map(ToString::to_string));

        Self {
            method,
            call_id,
            branch,
        }
    }
}

/// A transaction as seen from the server (the participant receiving the request)
pub struct ServerTransaction {
    pub request: Request,
    responses: mpsc::Sender<Response>,
}

impl ServerTransaction {
    pub fn respond(&mut self, status_code: StatusCode) -> ResponseBuilder<'_> {
        let headers = self.request.headers.iter()
            .filter(|&header| matches!(header, Header::Via(_) | Header::From(_) | Header::To(_) | Header::CSeq(_) | Header::CallId(_)))
            .cloned()
            .collect::<Vec<_>>();

        let builder = ResponseBuilder {
            tx: self,
            status_code,
            headers: headers.into(),
        };

        let builder = builder.header(rsip::headers::UserAgent::new(format!(
            "ucware-cli/{version}",
            version = env!("CARGO_PKG_VERSION")
        )));

        builder
    }
}

pub struct ResponseBuilder<'tx> {
    tx: &'tx ServerTransaction,
    status_code: StatusCode,
    headers: Headers,
}

impl<'tx> ResponseBuilder<'tx> {
    pub fn header(mut self, header: impl Into<Header>) -> Self {
        self.headers.push(header.into());
        self
    }

    pub async fn send(self, body: impl Into<Vec<u8>>) {
        let response = Response {
            status_code: self.status_code,
            version: Version::V2,
            headers: self.headers,
            body: body.into(),
        };

        self.tx.responses.send(response).await.expect("responses receiver closed");
    }
}

/// A transaction as seen from the client (the participant sending the request)
pub struct ClientTransaction {
    request: Request,

    responses: mpsc::Receiver<Response>,

    transactions: Weak<DashMap<TransactionKey, mpsc::Sender<Response>>>,
}

impl ClientTransaction {
    pub async fn receive(mut self) -> Result<Response> {
        loop {
            let Some(response) = self.responses.recv().await else {
                bail!("Transaction closed without response");
            };

            if response.status_code.kind() == StatusCodeKind::Provisional {
                continue;
            }

            return Ok(response);
        }
    }
}

impl Drop for ClientTransaction {
    fn drop(&mut self) {
        let Some(transactions) = self.transactions.upgrade() else {
            return;
        };

        transactions.remove(&TransactionKey::from_request(&self.request));
    }
}

pub struct Connection {
    url: Url,

    user: Uri,
    send_by: HostWithPort,

    sender: mpsc::Sender<Request>,

    transactions: Arc<DashMap<TransactionKey, mpsc::Sender<Response>>>,
}

impl Connection {
    pub async fn connect(
        url: Url,
        username: &str,
    ) -> Result<(Self, mpsc::Receiver<ServerTransaction>)> {
        info!("Connecting to: {url}");

        let mut request = url.clone().into_client_request()?;
        request.headers_mut().append(
            "Sec-WebSocket-Protocol",
            "sip".parse().expect("valid header value"),
        );

        let (stream, _response) = tokio_tungstenite::connect_async(request).await?;

        let (proto_tx, proto_rx) = stream.split();
        let proto_tx = proto_tx.sink_map_err(anyhow::Error::from);
        let proto_rx = proto_rx.map_err(anyhow::Error::from).fuse();

        let send_by = HostWithPort::from(Host::from(format!(
            "{}.invalid",
            Alphanumeric.sample_string(&mut rand::rng(), 16)
        )));

        let user = Uri {
            scheme: Some(Scheme::Sip),
            auth: Some(Auth {
                user: username.to_string(),
                password: None,
            }),
            host_with_port: HostWithPort {
                host: Host::from(url.domain().expect("URL must have domain")),
                port: None,
            },
            params: Default::default(),
            headers: Default::default(),
        };

        let transactions = Arc::new(DashMap::<TransactionKey, mpsc::Sender<Response>>::new());

        let (receiver_tx, receiver_rx) = mpsc::channel(1);
        let (sender_tx, sender_rx) = mpsc::channel(1);

        tokio::spawn(Self::run(
            proto_tx,
            proto_rx,
            sender_rx,
            receiver_tx,
            transactions.clone(),
        ));

        Ok((
            Self {
                url,
                user,
                send_by,
                sender: sender_tx,
                transactions,
            },
            receiver_rx,
        ))
    }

    async fn run(
        mut proto_tx: impl Sink<Message, Error = anyhow::Error> + Unpin,
        mut proto_rx: impl Stream<Item = Result<Message>> + Unpin,
        mut sender_rx: mpsc::Receiver<Request>,
        receiver_tx: mpsc::Sender<ServerTransaction>,
        transactions: Arc<DashMap<TransactionKey, mpsc::Sender<Response>>>,
    ) -> Result<()> {
        let (sender_res_tx, mut sender_res_rx) = mpsc::channel(1);

        loop {
            select! {
                msg = proto_rx.next() => {
                    trace!("Got message from WS: {msg:?}");

                    let Some(msg) = msg else {
                        return Ok(());
                    };

                    let msg = msg?;

                    match msg {
                        Message::Text(msg) => {
                            let msg = SipMessage::try_from(msg.as_str())?;
                            match msg {
                                SipMessage::Request(request) => {
                                    // Got a new request starting a new transaction
                                    let tx = ServerTransaction {
                                        request,
                                        responses: sender_res_tx.clone(),
                                    };

                                    receiver_tx.send(tx).await.expect("Request handler available");
                                }

                                SipMessage::Response(response) => {
                                    // Got a response - try to find according transaction
                                    let tx_key = TransactionKey::from_response(&response);
                                    let tx = transactions.get(&tx_key);
                                    let Some(tx) = tx else {
                                        warn!("Received response without matching transaction: {response:?}");
                                        continue;
                                    };

                                    tx.send(response).await.expect("Traansaction response handler available");
                                }
                            }
                        }

                        Message::Ping(payload) => {
                            proto_tx.send(Message::Pong(payload)).await?;
                            continue;
                        }

                        Message::Close(_) => {
                            return Ok(());
                        }

                        _ => {}
                    }
                }

                Some(msg) = sender_rx.recv() => {
                    trace!("Outgoing msg(request): {msg:?}");
                    let msg = Message::text(String::from(msg));
                    proto_tx.send(msg).await?;
                }

                Some(msg) = sender_res_rx.recv() => {
                    trace!("Outgoing msg(response): {msg:?}");
                    let msg = Message::text(String::from(msg));
                    proto_tx.send(msg).await?;
                }
            }
        }
    }

    pub async fn send(&self, request: Request) -> Result<ClientTransaction> {
        let (tx, rx) = mpsc::channel(1);

        let t = ClientTransaction {
            request: request.clone(),
            responses: rx,
            transactions: Arc::downgrade(&self.transactions),
        };

        let tx_key = TransactionKey::from_request(&t.request);
        trace!("Register transaction with: {tx_key:?}");

        self.transactions
            .insert(tx_key, tx);

        self.sender.send(request).await.expect("Request pipe closed");

        Ok(t)
    }

    pub fn dialog(&self) -> Dialog<'_> {
        let call_id = Alphanumeric.sample_string(&mut rand::rng(), 16);
        let seq = AtomicU32::new(rand::random::<u16>() as u32);

        Dialog {
            connection: self,
            call_id,
            seq,
        }
    }

    pub async fn register(&mut self, username: &str, password: &str) -> Result<()> {
        let contact = Alphanumeric.sample_string(&mut rand::rng(), 16);

        let dialog = self.dialog();

        let response = dialog
            .request(Method::Register)
            .send([])
            .await?
            .receive()
            .await?;

        if response.status_code.kind() == StatusCodeKind::Successful {
            return Ok(());
        }

        if response.status_code != StatusCode::Unauthorized {
            bail!("Failed to register: {}", response.status_code);
        }

        let authenticate = response
            .www_authenticate_header()
            .ok_or_else(|| anyhow!("No 'WWW-Authenticate' header received"))?
            .typed()?;

        let response = DigestGenerator {
            username,
            password,
            nonce: authenticate.nonce.as_str(),
            uri: &Default::default(),
            realm: authenticate.realm.as_str(),
            method: &Method::Register,
            qop: None,
            algorithm: authenticate.algorithm.unwrap_or(Algorithm::Md5),
        }
        .compute();

        let authorization = rsip::headers::typed::Authorization {
            scheme: auth::Scheme::Digest,
            username: username.to_string(),
            realm: authenticate.realm,
            nonce: authenticate.nonce,
            uri: Default::default(),
            response,
            algorithm: authenticate.algorithm,
            opaque: authenticate.opaque,
            qop: None,
        };

        let response = dialog
            .request(Method::Register)
            .header(rsip::headers::typed::Contact {
                display_name: None,
                uri: Uri {
                    scheme: Some(Scheme::Sip),
                    auth: Some(Auth {
                        user: contact.clone(),
                        password: None,
                    }),
                    host_with_port: self.send_by.clone(),
                    params: vec![Param::Transport(Transport::Ws)],
                    headers: vec![],
                },
                params: vec![Param::Expires("6000".into())],
            })
            .header(authorization)
            .send([])
            .await?
            .receive()
            .await?;

        if response.status_code.kind() != StatusCodeKind::Successful {
            bail!("Failed to register: {}", response.status_code);
        }

        Ok(())
    }
}

pub struct Dialog<'c> {
    connection: &'c Connection,

    call_id: String,
    seq: AtomicU32,
}

impl<'c> Dialog<'c> {
    pub fn request(&self, method: Method) -> RequestBuilder<'c, '_> {
        let builder = RequestBuilder {
            dialog: self,
            method,
            headers: Default::default(),
        };

        let builder = builder.header(rsip::headers::typed::Via {
            version: Version::V2,
            transport: Transport::Wss,
            uri: Uri::from(self.connection.send_by.clone()),
            params: vec![],
        });

        let builder = builder.header(rsip::headers::typed::To {
            display_name: None,
            uri: self.connection.user.clone(),
            params: Default::default(),
        });

        let builder = builder.header(rsip::headers::typed::From {
            display_name: None,
            uri: self.connection.user.clone(),
            params: vec![],
        });

        let builder = builder.header(rsip::headers::typed::CSeq {
            seq: self.seq.fetch_add(1, Ordering::Release),
            method,
        });

        let builder = builder.header(CallId::new(self.call_id.clone()));

        let builder = builder.header(rsip::headers::UserAgent::new(format!(
            "ucware-cli/{version}",
            version = env!("CARGO_PKG_VERSION")
        )));

        builder
    }
}

pub struct RequestBuilder<'c, 'd> {
    dialog: &'d Dialog<'c>,

    method: Method,
    headers: Headers,
}

impl<'c, 'd> RequestBuilder<'c, 'd> {
    pub fn header(mut self, header: impl Into<Header>) -> Self {
        self.headers.push(header.into());
        self
    }

    pub async fn send(self, body: impl Into<Vec<u8>>) -> Result<ClientTransaction> {
        let request = Request {
            method: self.method,
            uri: Uri {
                scheme: Some(Scheme::Sip),
                auth: None,
                host_with_port: Host::from(
                    self.dialog.connection.url.domain().expect("URL must have domain"),
                )
                .into(),
                params: Vec::default(),
                headers: Vec::default(),
            },
            headers: self.headers,
            version: Version::V2,
            body: body.into(),
        };

        trace!("Sending request: {request:#?}");

        self.dialog.connection.send(request).await
    }
}
