use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_HEADERS_SIZE: usize = 8000;
const MAX_BODY_SIZE: usize = 10000000;
const MAX_NUM_HEADERS: usize = 32;

#[derive(Debug)]
pub enum Error {
    /// 客户端在发送完整请求之前挂断
    IncompleteResponse,
    /// 客户端发送了无效的 HTTP 请求。httparse::Error 包含更多详细信息
    MalformedResponse(httparse::Error),
    /// Content-Length 头存在，但不包含有效的数字值
    InvalidContentLength,
    /// Content-Length 头与发送的请求体大小不匹配
    ContentLengthMismatch,
    /// 请求体大于 MAX_BODY_SIZE
    ResponseBodyTooLarge,
    /// 读取/写入 TcpStream 时遇到 I/O 错误
    ConnectionError(std::io::Error),
}

/// 从提供的响应中提取 Content-Length 头值。如果 Content-Length 存在且有效则返回 Ok(Some(usize))，
/// 如果 Content-Length 不存在则返回 Ok(None)，如果 Content-Length 存在但无效则返回 Err(Error)。
///
/// 您不需要修改此函数。
fn get_content_length(response: &http::Response<Vec<u8>>) -> Result<Option<usize>, Error> {
    // 查找 content-length 头
    if let Some(header_value) = response.headers().get("content-length") {
        // 如果存在，将其解析为 usize（如果无法解析则返回 InvalidResponseFormat）
        Ok(Some(
            header_value
                .to_str()
                .or(Err(Error::InvalidContentLength))?
                .parse::<usize>()
                .or(Err(Error::InvalidContentLength))?,
        ))
    } else {
        // 如果不存在，返回 None
        Ok(None)
    }
}

/// 尝试将提供的缓冲区中的数据解析为 HTTP 响应。返回以下之一：
///
/// * 如果缓冲区中有完整且有效的响应，返回 Ok(Some(http::Request))
/// * 如果缓冲区中有不完整但到目前为止有效的响应，返回 Ok(None)
/// * 如果缓冲区中的数据绝对不是有效的 HTTP 响应，返回 Err(Error)
///
/// 您不需要修改此函数。
fn parse_response(buffer: &[u8]) -> Result<Option<(http::Response<Vec<u8>>, usize)>, Error> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_NUM_HEADERS];
    let mut resp = httparse::Response::new(&mut headers);
    let res = resp
        .parse(buffer)
        .or_else(|err| Err(Error::MalformedResponse(err)))?;

    if let httparse::Status::Complete(len) = res {
        let mut response = http::Response::builder()
            .status(resp.code.unwrap())
            .version(http::Version::HTTP_11);
        for header in resp.headers {
            response = response.header(header.name, header.value);
        }
        let response = response.body(Vec::new()).unwrap();
        Ok(Some((response, len)))
    } else {
        Ok(None)
    }
}

/// 从提供的流中读取 HTTP 响应，等待直到发送完整的头集合。
/// 此函数只读取响应行和头；随后可以调用 read_body 函数来读取响应体。
///
/// 如果收到有效响应则返回 Ok(http::Response)，否则返回 Error。
///
/// 您需要在里程碑 2 中修改此函数。
async fn read_headers(stream: &mut TcpStream) -> Result<http::Response<Vec<u8>>, Error> {
    // 尝试从响应中读取头。我们可能不会一次收到所有头
    // （例如，我们可能先收到响应的前几个字节，然后其余部分稍后到达）。
    // 反复尝试解析，直到我们读取到有效的 HTTP 响应
    let mut response_buffer = [0_u8; MAX_HEADERS_SIZE];
    let mut bytes_read = 0;
    loop {
        // 从连接中读取字节到缓冲区，从 bytes_read 位置开始
        let new_bytes = stream
            .read(&mut response_buffer[bytes_read..])
            .await
            .or_else(|err| Err(Error::ConnectionError(err)))?;
        if new_bytes == 0 {
            // 我们没能读取到完整的响应
            return Err(Error::IncompleteResponse);
        }
        bytes_read += new_bytes;

        // 查看我们到目前为止是否已读取到有效响应
        if let Some((mut response, headers_len)) = parse_response(&response_buffer[..bytes_read])? {
            // 我们已读取了完整的头集合。我们可能还读取了响应体的第一部分；
            // 取出响应缓冲区中剩余的内容，并将其保存为响应体的开始。
            response
                .body_mut()
                .extend_from_slice(&response_buffer[headers_len..bytes_read]);
            return Ok(response);
        }
    }
}

/// 此函数从流中读取响应的响应体。如果存在 Content-Length 头，则读取相应字节数；
/// 否则，读取字节直到连接关闭。
///
/// 您需要在里程碑 2 中修改此函数。
async fn read_body(stream: &mut TcpStream, response: &mut http::Response<Vec<u8>>) -> Result<(), Error> {
    // 响应可能提供也可能不提供 Content-Length 头。如果提供了该头，则我们
    // 要读取相应字节数；如果没有提供，我们要持续读取字节直到连接关闭。
    let content_length = get_content_length(response)?;

    while content_length.is_none() || response.body().len() < content_length.unwrap() {
        let mut buffer = [0_u8; 512];
        let bytes_read = stream
            .read(&mut buffer)
            .await
            .or_else(|err| Err(Error::ConnectionError(err)))?;
        if bytes_read == 0 {
            // 服务器已挂断！
            if content_length.is_none() {
                // 我们已到达响应的末尾
                break;
            } else {
                // Content-Length 已设置，但服务器在我们读取相应字节数之前挂断了
                return Err(Error::ContentLengthMismatch);
            }
        }

        // 确保服务器发送的字节数不超过它承诺发送的字节数
        if content_length.is_some() && response.body().len() + bytes_read > content_length.unwrap()
        {
            return Err(Error::ContentLengthMismatch);
        }

        // 确保服务器发送的字节数不超过我们允许的字节数
        if response.body().len() + bytes_read > MAX_BODY_SIZE {
            return Err(Error::ResponseBodyTooLarge);
        }

        // 将接收到的字节追加到响应体
        response.body_mut().extend_from_slice(&buffer[..bytes_read]);
    }
    Ok(())
}

/// 此函数从流中读取并返回 HTTP 响应，如果服务器过早关闭连接或发送无效响应则返回 Error。
///
/// 您需要在里程碑 2 中修改此函数。
pub async fn read_from_stream(
    stream: &mut TcpStream,
    request_method: &http::Method,
) -> Result<http::Response<Vec<u8>>, Error> {
    let mut response = read_headers(stream).await?;
    // 只要响应不是对 HEAD 请求的响应，并且响应状态码不是 1xx、204（无内容）或 304（未修改），
    // 响应就可能有响应体。
    if !(request_method == http::Method::HEAD
        || response.status().as_u16() < 200
        || response.status() == http::StatusCode::NO_CONTENT
        || response.status() == http::StatusCode::NOT_MODIFIED)
    {
        read_body(stream, &mut response).await?;
    }
    Ok(response)
}

/// 此函数将响应序列化为字节并将这些字节写入提供的流。
///
/// 您需要在里程碑 2 中修改此函数。
pub async fn write_to_stream(
    response: &http::Response<Vec<u8>>,
    stream: &mut TcpStream,
) -> Result<(), std::io::Error> {
    stream.write_all(&format_response_line(response).into_bytes()).await?;
    stream.write_all(&['\r' as u8, '\n' as u8]).await?; // \r\n
    for (header_name, header_value) in response.headers() {
        stream.write_all(&format!("{}: ", header_name).as_bytes()).await?;
        stream.write_all(header_value.as_bytes()).await?;
        stream.write_all(&['\r' as u8, '\n' as u8]).await?; // \r\n
    }
    stream.write_all(&['\r' as u8, '\n' as u8]).await?;
    if response.body().len() > 0 {
        stream.write_all(response.body()).await?;
    }
    Ok(())
}

pub fn format_response_line(response: &http::Response<Vec<u8>>) -> String {
    format!(
        "{:?} {} {}",
        response.version(),
        response.status().as_str(),
        response.status().canonical_reason().unwrap_or("")
    )
}

/// 这是一个辅助函数，创建包含可以发送给客户端的 HTTP 错误的 http::Response。
pub fn make_http_error(status: http::StatusCode) -> http::Response<Vec<u8>> {
    let body = format!(
        "HTTP {} {}",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .into_bytes();
    http::Response::builder()
        .status(status)
        .header("Content-Type", "text/plain")
        .header("Content-Length", body.len().to_string())
        .version(http::Version::HTTP_11)
        .body(body)
        .unwrap()
}
