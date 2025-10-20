use std::cmp::min;
use std::io::{Read, Write};
use std::net::TcpStream;

const MAX_HEADERS_SIZE: usize = 8000;
const MAX_BODY_SIZE: usize = 10000000;
const MAX_NUM_HEADERS: usize = 32;

#[derive(Debug)]
pub enum Error {
    /// 客户端在发送完整请求之前挂断。IncompleteRequest 包含客户端挂断前成功读取的字节数
    IncompleteRequest(usize),
    /// 客户端发送了无效的 HTTP 请求。httparse::Error 包含更多详细信息
    MalformedRequest(httparse::Error),
    /// Content-Length 头存在，但不包含有效的数字值
    InvalidContentLength,
    /// Content-Length 头与发送的请求体大小不匹配
    ContentLengthMismatch,
    /// 请求体大于 MAX_BODY_SIZE
    RequestBodyTooLarge,
    /// 读取/写入 TcpStream 时遇到 I/O 错误
    ConnectionError(std::io::Error),
}

/// 从提供的请求中提取 Content-Length 头值。如果 Content-Length 存在且有效则返回 Ok(Some(usize))，
/// 如果 Content-Length 不存在则返回 Ok(None)，如果 Content-Length 存在但无效则返回 Err(Error)。
///
/// 您不需要修改此函数。
fn get_content_length(request: &http::Request<Vec<u8>>) -> Result<Option<usize>, Error> {
    // 查找 content-length 头
    if let Some(header_value) = request.headers().get("content-length") {
        // 如果存在，将其解析为 usize（如果无法解析则返回 InvalidContentLength）
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

/// 此函数追加到头值（如果头尚不存在则添加新头）。这用于将客户端的 IP 地址添加到 
/// X-Forwarded-For 列表的末尾，或者如果尚不存在则添加新的 X-Forwarded-For 头。
///
/// 您不需要修改此函数。
pub fn extend_header_value(
    request: &mut http::Request<Vec<u8>>,
    name: &'static str,
    extend_value: &str,
) {
    let new_value = match request.headers().get(name) {
        Some(existing_value) => {
            [existing_value.as_bytes(), b", ", extend_value.as_bytes()].concat()
        }
        None => extend_value.as_bytes().to_owned(),
    };
    request
        .headers_mut()
        .insert(name, http::HeaderValue::from_bytes(&new_value).unwrap());
}

/// 尝试将提供的缓冲区中的数据解析为 HTTP 请求。返回以下之一：
///
/// * 如果缓冲区中有完整且有效的请求，返回 Ok(Some(http::Request))
/// * 如果缓冲区中有不完整但到目前为止有效的请求，返回 Ok(None)
/// * 如果缓冲区中的数据绝对不是有效的 HTTP 请求，返回 Err(Error)
///
/// 您不需要修改此函数。
fn parse_request(buffer: &[u8]) -> Result<Option<(http::Request<Vec<u8>>, usize)>, Error> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_NUM_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    let res = req.parse(buffer).or_else(|err| Err(Error::MalformedRequest(err)))?;

    if let httparse::Status::Complete(len) = res {
        let mut request = http::Request::builder()
            .method(req.method.unwrap())
            .uri(req.path.unwrap())
            .version(http::Version::HTTP_11);
        for header in req.headers {
            request = request.header(header.name, header.value);
        }
        let request = request.body(Vec::new()).unwrap();
        Ok(Some((request, len)))
    } else {
        Ok(None)
    }
}

/// 从提供的流中读取 HTTP 请求，等待直到发送完整的头集合。
/// 此函数只读取请求行和头；随后可以调用 read_body 函数来读取请求体（对于 POST 请求）。
///
/// 如果收到有效请求则返回 Ok(http::Request)，否则返回 Error。
///
/// 您需要在里程碑 2 中修改此函数。
fn read_headers(stream: &mut TcpStream) -> Result<http::Request<Vec<u8>>, Error> {
    // 尝试从请求中读取头。我们可能不会一次收到所有头
    // （例如，我们可能先收到请求的前几个字节，然后其余部分稍后到达）。
    // 反复尝试解析，直到我们读取到有效的 HTTP 请求
    let mut request_buffer = [0_u8; MAX_HEADERS_SIZE];
    let mut bytes_read = 0;
    loop {
        // 从连接中读取字节到缓冲区，从 bytes_read 位置开始
        let new_bytes = stream
            .read(&mut request_buffer[bytes_read..])
            .or_else(|err| Err(Error::ConnectionError(err)))?;
        if new_bytes == 0 {
            // 我们没能读取到完整的请求
            return Err(Error::IncompleteRequest(bytes_read));
        }
        bytes_read += new_bytes;

        // 查看我们到目前为止是否已读取到有效请求
        if let Some((mut request, headers_len)) = parse_request(&request_buffer[..bytes_read])? {
            // 我们已读取了完整的头集合。但是，如果这是 POST 请求，可能还包含了请求体，
            // 并且我们可能已经从流中将部分请求体读取到了 header_buffer 中。我们需要将这些字节
            // 添加到 Request body 中，以免丢失它们
            request
                .body_mut()
                .extend_from_slice(&request_buffer[headers_len..bytes_read]);
            return Ok(request);
        }
    }
}

/// 此函数从流中读取请求的请求体。只有当 Content-Length 头存在时，客户端才会发送请求体；
/// 此函数从流中读取相应字节数。如果成功则返回 Ok(())，如果无法读取 Content-Length 字节数则返回 Err(Error)。
///
/// 您需要在里程碑 2 中修改此函数。
fn read_body(
    stream: &mut TcpStream,
    request: &mut http::Request<Vec<u8>>,
    content_length: usize,
) -> Result<(), Error> {
    // 持续读取数据，直到我们读取了完整的请求体长度，或者遇到错误。
    while request.body().len() < content_length {
        // 一次最多读取 512 字节。（如果客户端只发送了小的请求体，则只分配读取该请求体所需的空间。）
        let mut buffer = vec![0_u8; min(512, content_length)];
        let bytes_read = stream.read(&mut buffer).or_else(|err| Err(Error::ConnectionError(err)))?;

        // 确保客户端仍在向我们发送字节
        if bytes_read == 0 {
            log::debug!(
                "Client hung up after sending a body of length {}, even though it said the content \
                length is {}",
                request.body().len(),
                content_length
            );
            return Err(Error::ContentLengthMismatch);
        }

        // 确保客户端没有发送*过多*的字节
        if request.body().len() + bytes_read > content_length {
            log::debug!(
                "Client sent more bytes than we expected based on the given content length!"
            );
            return Err(Error::ContentLengthMismatch);
        }

        // 将接收到的字节存储到请求体中
        request.body_mut().extend_from_slice(&buffer[..bytes_read]);
    }
    Ok(())
}

/// 此函数从流中读取并返回 HTTP 请求，如果客户端过早关闭连接或发送无效请求则返回 Error。
///
/// 您需要在里程碑 2 中修改此函数。
pub fn read_from_stream(stream: &mut TcpStream) -> Result<http::Request<Vec<u8>>, Error> {
    // 读取头
    let mut request = read_headers(stream)?;
    // 如果客户端提供了 Content-Length 头（对于 POST 请求会提供），则读取请求体
    if let Some(content_length) = get_content_length(&request)? {
        if content_length > MAX_BODY_SIZE {
            return Err(Error::RequestBodyTooLarge);
        } else {
            read_body(stream, &mut request, content_length)?;
        }
    }
    Ok(request)
}

/// 此函数将请求序列化为字节并将这些字节写入提供的流。
///
/// 您需要在里程碑 2 中修改此函数。
pub fn write_to_stream(
    request: &http::Request<Vec<u8>>,
    stream: &mut TcpStream,
) -> Result<(), std::io::Error> {
    stream.write(&format_request_line(request).into_bytes())?;
    stream.write(&['\r' as u8, '\n' as u8])?; // \r\n
    for (header_name, header_value) in request.headers() {
        stream.write(&format!("{}: ", header_name).as_bytes())?;
        stream.write(header_value.as_bytes())?;
        stream.write(&['\r' as u8, '\n' as u8])?; // \r\n
    }
    stream.write(&['\r' as u8, '\n' as u8])?;
    if request.body().len() > 0 {
        stream.write(request.body())?;
    }
    Ok(())
}

pub fn format_request_line(request: &http::Request<Vec<u8>>) -> String {
    format!("{} {} {:?}", request.method(), request.uri(), request.version())
}
