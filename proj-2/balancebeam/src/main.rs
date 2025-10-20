mod request;
mod response;

use clap::Parser;
use rand::{Rng, SeedableRng};
use std::net::{TcpListener, TcpStream};

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
}

fn main() {
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
    let listener = match TcpListener::bind(&options.bind) {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Could not bind to {}: {}", options.bind, err);
            std::process::exit(1);
        }
    };
    log::info!("Listening for requests on {}", options.bind);

    // 处理传入的连接
    let state = ProxyState {
        upstream_addresses: options.upstream,
        active_health_check_interval: options.active_health_check_interval,
        active_health_check_path: options.active_health_check_path,
        max_requests_per_minute: options.max_requests_per_minute,
    };
    for stream in listener.incoming() {
        if let Ok(stream) = stream {
            // 处理连接！
            handle_connection(stream, &state);
        }
    }
}

fn connect_to_upstream(state: &ProxyState) -> Result<TcpStream, std::io::Error> {
    let mut rng = rand::rngs::StdRng::from_entropy();
    let upstream_idx = rng.gen_range(0, state.upstream_addresses.len());
    let upstream_ip = &state.upstream_addresses[upstream_idx];
    TcpStream::connect(upstream_ip).or_else(|err| {
        log::error!("Failed to connect to upstream {}: {}", upstream_ip, err);
        Err(err)
    })
    // TODO: 实现故障转移（里程碑 3）
}

fn send_response(client_conn: &mut TcpStream, response: &http::Response<Vec<u8>>) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("{} <- {}", client_ip, response::format_response_line(&response));
    if let Err(error) = response::write_to_stream(&response, client_conn) {
        log::warn!("Failed to send response to client: {}", error);
        return;
    }
}

fn handle_connection(mut client_conn: TcpStream, state: &ProxyState) {
    let client_ip = client_conn.peer_addr().unwrap().ip().to_string();
    log::info!("Connection received from {}", client_ip);

    // 打开与随机目标服务器的连接
    let mut upstream_conn = match connect_to_upstream(state) {
        Ok(stream) => stream,
        Err(_error) => {
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response);
            return;
        }
    };
    let upstream_ip = client_conn.peer_addr().unwrap().ip().to_string();

    // 客户端现在可能会向我们发送一个或多个请求。继续尝试读取请求，直到客户端挂断或我们遇到错误。
    loop {
        // 从客户端读取请求
        let mut request = match request::read_from_stream(&mut client_conn) {
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
                send_response(&mut client_conn, &response);
                continue;
            }
        };
        log::info!(
            "{} -> {}: {}",
            client_ip,
            upstream_ip,
            request::format_request_line(&request)
        );

        // 添加 X-Forwarded-For 头，以便上游服务器知道客户端的 IP 地址。
        // （我们直接连接到上游服务器，所以没有这个头的话，上游服务器只知道我们的 IP，而不知道客户端的 IP。）
        request::extend_header_value(&mut request, "x-forwarded-for", &client_ip);

        // 将请求转发到服务器
        if let Err(error) = request::write_to_stream(&request, &mut upstream_conn) {
            log::error!("Failed to send request to upstream {}: {}", upstream_ip, error);
            let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
            send_response(&mut client_conn, &response);
            return;
        }
        log::debug!("Forwarded request to server");

        // 读取服务器的响应
        let response = match response::read_from_stream(&mut upstream_conn, request.method()) {
            Ok(response) => response,
            Err(error) => {
                log::error!("Error reading response from server: {:?}", error);
                let response = response::make_http_error(http::StatusCode::BAD_GATEWAY);
                send_response(&mut client_conn, &response);
                return;
            }
        };
        // 将响应转发给客户端
        send_response(&mut client_conn, &response);
        log::debug!("Forwarded response to client");
    }
}
