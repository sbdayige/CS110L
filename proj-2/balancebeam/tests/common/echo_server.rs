use crate::common::server::Server;
use async_trait::async_trait;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming as IncomingBody, Request, Response};
use hyper_util::rt::TokioIo;
use http_body_util::{BodyExt, Full};
use bytes::Bytes;
use rand::Rng;
use std::sync::{atomic, Arc};
use tokio::sync::oneshot;
use tokio::net::TcpListener;

#[derive(Debug)]
struct ServerState {
    pub requests_received: atomic::AtomicUsize,
}

async fn echo(
    server_state: Arc<ServerState>,
    req: Request<IncomingBody>,
) -> Result<Response<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
    server_state
        .requests_received
        .fetch_add(1, atomic::Ordering::SeqCst);
    let mut req_text = format!("{} {} {:?}\n", req.method(), req.uri(), req.version());
    for (header_name, header_value) in req.headers() {
        req_text += &format!(
            "{}: {}\n",
            header_name.as_str(),
            header_value.to_str().unwrap_or("<binary value>")
        );
    }
    req_text += "\n";
    let mut req_as_bytes = req_text.into_bytes();
    let body_bytes = req.into_body().collect().await?.to_bytes();
    req_as_bytes.extend_from_slice(&body_bytes);
    Ok(Response::new(Full::new(Bytes::from(req_as_bytes))))
}

pub struct EchoServer {
    shutdown_signal_sender: oneshot::Sender<()>,
    server_task: tokio::task::JoinHandle<()>,
    pub address: String,
    state: Arc<ServerState>,
}

impl EchoServer {
    pub async fn new() -> EchoServer {
        let mut rng = rand::thread_rng();
        EchoServer::new_at_address(format!("127.0.0.1:{}", rng.gen_range(1024..65535))).await
    }

    pub async fn new_at_address(bind_addr_string: String) -> EchoServer {
        // Create a one-shot channel that can be used to tell the server to shut down
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        // Start a separate server task
        let server_state = Arc::new(ServerState {
            requests_received: atomic::AtomicUsize::new(0),
        });
        let server_task_state = server_state.clone();
        
        let listener = TcpListener::bind(&bind_addr_string).await.unwrap();
        
        let server_task = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let server_task_state = server_task_state.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |req| {
                                        let server_task_state = server_task_state.clone();
                                        echo(server_task_state, req)
                                    });
                                    if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                                        log::error!("Error serving connection: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                log::error!("Error accepting connection: {}", e);
                            }
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });

        EchoServer {
            shutdown_signal_sender: shutdown_tx,
            server_task,
            state: server_state,
            address: bind_addr_string,
        }
    }
}

#[async_trait]
impl Server for EchoServer {
    async fn stop(self: Box<Self>) -> usize {
        // Tell the hyper server to stop
        let _ = self.shutdown_signal_sender.send(());
        // Wait for it to stop
        self.server_task
            .await
            .expect("ErrorServer server task panicked");

        self.state.requests_received.load(atomic::Ordering::SeqCst)
    }

    fn address(&self) -> String {
        self.address.clone()
    }
}
