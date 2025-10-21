mod request;
mod response;

use clap::Parser;
use rand::{Rng, SeedableRng};
use tokio::net::{TcpListener, TcpStream};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashSet;
use std::time::Duration;
use tokio::time::timeout;

/// 包含从命令行调用 balancebeam 时解析的信息。Clap 宏提供了一种自动构建命令行参数解析器的便捷方式。
#[derive(Parser, Debug)]
#[clap(about = "Fun with load balancing")]
struct CmdOptions {
    #[clap(
        short,
        long,
        help = "IP/port to bind to",
        default_value = "0.0.0.0:1100"
    )]
    bind: String,
    #[clap(short, long, help = "Upstream host to forward requests to")]
    upstream: Vec<String>,
    #[clap(
        long,
        help = "Perform active health checks on this interval (in seconds)",
        default_value = "10"
    )]
    active_health_check_interval: usize,
    #[clap(
    long,
    help = "Path to send request to for active health checks",
    default_value = "/"
    )]
    active_health_check_path: String,
    #[clap(
        long,
        help = "Maximum number of requests to accept per IP per minute (0 = unlimited)",
        default_value = "0"
    )]
    max_requests_per_minute: usize,
    #[clap(
        long,
        help = "Number of worker threads in the thread pool",
        default_value = "4"
    )]
    num_threads: usize,
}

/// 包含有关 balancebeam 状态的信息（例如，我们当前代理到哪些服务器，哪些服务器失败了，速率限制计数等）
///
/// 您应该在后续里程碑中向此结构体添加字段。
struct ProxyState {
    /// 检查上游服务器是否存活的频率（里程碑 4）
    #[allow(dead_code)]
    active_health_check_interval: usize,
    /// 执行主动健康检查时应该发送请求的路径（里程碑 4）
    #[allow(dead_code)]
    active_health_check_path: String,
    /// 单个 IP 在一分钟内可以发出的最大请求数（里程碑 5）
    #[allow(dead_code)]
    max_requests_per_minute: usize,
    /// 我们正在代理到的服务器地址
    upstream_addresses: Vec<String>,
    /// 存储已失败的上游服务器索引（里程碑 3）
    /// 使用 RwLock 允许多个任务同时读取，只有在标记服务器失败时才需要写锁
    dead_upstreams: RwLock<HashSet<usize>>,
}

#[tokio::main]
async fn main() {
    // 初始化日志库。您可以使用 `log` 宏打印日志消息：
    // https://docs.rs/log/0.4.8/log/ 您也可以继续使用 print! 语句；这只是看起来更美观一些。
    if let Err(_) = std::env::var("RUST_LOG") {
        std::env::set_var("RUST_LOG", "debug");
    }
    pretty_env_logger::init();

    // 解析传递给该程序的命令行参数
    let options = CmdOptions::parse();
    if options.upstream.len() < 1 {
        log::error!("At least one upstream server must be specified using the --upstream option.");
        std::process::exit(1);
    }

    // 开始监听连接
    let listener = match TcpListener::bind(&options.bind).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    // 处理传入的连接
    let state = Arc::new(ProxyState {
        upstream_addresses: options.upstream,
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
        dead_upstreams: RwLock::new(HashSet::new()),
    });
    
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let state = Arc::clone(&state);
                // 为每个连接spawn一个新的异步任务
                tokio::spawn(async move {
                    handle_connection(stream, &state).await;
                });
            }
            Err(err) => {
                log::error!("Error accepting connection: {}", err);
            }
        }
    }
}

/// 尝试连接到一个存活的上游服务器，如果选中的服务器失败则自动故障转移到其他服务器
/// 
/// 该函数实现被动健康检查：
/// 1. 首先从存活的服务器中随机选择一个
/// 2. 如果连接失败，将该服务器标记为失败
/// 3. 重试其他存活的服务器
/// 4. 如果所有服务器都失败，返回错误
async fn connect_to_upstream(state: &ProxyState) -> Result<(TcpStream, usize), std::io::Error> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    
    // 获取所有上游服务器的索引
    let total_upstreams = state.upstream_addresses.len();
    
    // 尝试连接到存活的服务器
    let mut tried_upstreams = HashSet::new();
    
    while tried_upstreams.len() < total_upstreams {
        // 每次重新读取失败服务器列表（确保获取最新状态）
        let dead_upstreams = state.dead_upstreams.read().await;
        
        // 构建存活且未尝试过的服务器索引列表
        let available_upstreams: Vec<usize> = (0..total_upstreams)
            .filter(|idx| !dead_upstreams.contains(idx) && !tried_upstreams.contains(idx))
            .collect();
        
        drop(dead_upstreams);
        
        // 如果没有可用的服务器，返回错误
        if available_upstreams.is_empty() {
            log::error!("No more available upstream servers to try!");
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "All upstream servers are dead or have been tried"
            ));
        }
        
        // 随机选择一个可用的服务器
        let random_idx = rng.gen_range(0..available_upstreams.len());
        let upstream_idx = available_upstreams[random_idx];
        let upstream_ip = &state.upstream_addresses[upstream_idx];
        
        tried_upstreams.insert(upstream_idx);
        
        log::debug!("Attempting to connect to upstream {} (index {})", upstream_ip, upstream_idx);
        
        // 设置连接超时为2秒
        let connect_result = timeout(
            Duration::from_secs(2),
            TcpStream::connect(upstream_ip)
        ).await;
        
        match connect_result {
            Ok(Ok(stream)) => {
                log::info!("Successfully connected to upstream {}", upstream_ip);
                return Ok((stream, upstream_idx));
            }
            Ok(Err(err)) => {
                log::warn!(
                    "Failed to connect to upstream {} (index {}): {}. Marking as dead.",
                    upstream_ip, upstream_idx, err
                );
                
                // 将该服务器标记为失败
                let mut dead_upstreams = state.dead_upstreams.write().await;
                dead_upstreams.insert(upstream_idx);
                drop(dead_upstreams);
                
                // 继续尝试其他服务器
                log::info!("Retrying with another upstream server...");
            }
            Err(_) => {
                // 超时
                log::warn!(
                    "Timeout connecting to upstream {} (index {}). Marking as dead.",
                    upstream_ip, upstream_idx
                );
                
                // 将该服务器标记为失败
                let mut dead_upstreams = state.dead_upstreams.write().await;
                dead_upstreams.insert(upstream_idx);
                drop(dead_upstreams);
                
                // 继续尝试其他服务器
                log::info!("Retrying with another upstream server...");
            }
        }
    }
    
    // 所有服务器都尝试过了
    log::error!("All upstream servers have failed!");
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "All upstream servers failed during connection attempts"
    ))
}

async fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("{} <- {}", client_ip, response::format_response_line(&response));
    if let Err(error) = response::write_to_stream(&response, client_conn).await {
        log::warn!("Failed to send response to client: {}", error);
        return;
    }
}

async fn handle_connection(mut client_conn: TcpStream, state: &ProxyState) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // 客户端现在可能会向我们发送一个或多个请求。继续尝试读取请求，直到客户端挂断或我们遇到错误。
    loop {
        // 从客户端读取请求
        let mut request = match request::read_from_stream(&mut client_conn).await {
            Ok(request) => request,
            // 处理客户端关闭连接且不再发送请求的情况
            Err(request::Error::IncompleteRequest(0)) => {
                log::debug!("Client finished sending requests. Shutting down connection");
                return;
            }
            // 处理从客户端读取时的 I/O 错误
            Err(request::Error::ConnectionError(io_err)) => {
                log::info!("Error reading request from client stream: {}", io_err);
                return;
            }
            Err(error) => {
                log::debug!("Error parsing request: {:?}", error);
                let response = response::make_http_error(match error {
                    request::Error::IncompleteRequest(_)
                    | request::Error::MalformedRequest(_)
                    | request::Error::InvalidContentLength
                    | request::Error::ContentLengthMismatch => http::StatusCode::BAD_REQUEST,
                    request::Error::RequestBodyTooLarge => http::StatusCode::PAYLOAD_TOO_LARGE,
                    request::Error::ConnectionError(_) => http::StatusCode::SERVICE_UNAVAILABLE,
                });
                send_response(&mut client_conn, &response).await;
                continue;
            }
        };
        log::info!(
            "{} -> {}",
            client_ip,
            request::format_request_line(&request)
        );

        // 添加 X-Forwarded-For 头
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        // 尝试将请求转发到上游服务器，如果失败则重试其他服务器
        let max_retries = state.upstream_addresses.len();
        let mut retry_count = 0;
        let mut success = false;
        
        while retry_count < max_retries && !success {
            retry_count += 1;
            log::debug!("Request forwarding attempt {} of {}", retry_count, max_retries);
            
            // 为每个请求建立新的上游连接
            let (mut upstream_conn, upstream_idx) = match connect_to_upstream(state).await {
                Ok((stream, idx)) => (stream, idx),
                Err(_error) => {
                    log::warn!("Failed to connect to any upstream server on attempt {}", retry_count);
                    if retry_count >= max_retries {
                        let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                        send_response(&mut client_conn, &response).await;
                        return;
                    }
                    continue;
                }
            };
            let upstream_ip = upstream_conn.peer_addr().unwrap().ip().to_string();
            log::info!("Forwarding request to upstream {}", upstream_ip);

            // 将请求转发到服务器
            if let Err(error) = request::write_to_stream(&request, &mut upstream_conn).await {
                log::error!("Failed to send request to upstream {}: {}", upstream_ip, error);
                drop(upstream_conn);
                // 标记这个upstream为失败
                let mut dead_upstreams = state.dead_upstreams.write().await;
                dead_upstreams.insert(upstream_idx);
                drop(dead_upstreams);
                continue; // 重试其他服务器
            }
            log::debug!("Forwarded request to server");

            // 读取服务器的响应（设置超时为1秒）
            let response_result = timeout(
                Duration::from_secs(1),
                response::read_from_stream(&mut upstream_conn, request.method())
            ).await;
            
            match response_result {
                Ok(Ok(response)) => {
                    // 成功读取响应
                    log::debug!("Received response from upstream");
                    send_response(&mut client_conn, &response).await;
                    log::debug!("Forwarded response to client");
                    drop(upstream_conn);
                    success = true;
                }
                Ok(Err(error)) => {
                    log::error!("Error reading response from server {}: {:?}", upstream_ip, error);
                    drop(upstream_conn);
                    // 标记这个upstream为失败
                    let mut dead_upstreams = state.dead_upstreams.write().await;
                    dead_upstreams.insert(upstream_idx);
                    drop(dead_upstreams);
                    // 重试其他服务器
                    continue;
                }
                Err(_) => {
                    log::error!("Timeout reading response from upstream {}", upstream_ip);
                    drop(upstream_conn);
                    // 标记这个upstream为失败
                    let mut dead_upstreams = state.dead_upstreams.write().await;
                    dead_upstreams.insert(upstream_idx);
                    drop(dead_upstreams);
                    // 重试其他服务器
                    continue;
                }
            }
        }
        
        // 如果所有重试都失败了
        if !success {
            log::error!("Failed to forward request after {} attempts", max_retries);
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response).await;
            return;
        }
    }
}