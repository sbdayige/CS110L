use crate::process::Process;
use nix::unistd::getuid;
use std::fmt;
use std::process::Command;

/// 这个枚举表示可能发生错误的原因。它很有用，因为它允许 API 的调用者根据出错的具体情况
/// 对错误处理进行细粒度控制。你可以在 Rust 库中找到类似的想法，例如 std::io:
/// https://doc.rust-lang.org/std/io/enum.ErrorKind.html 
/// 不过，你不需要在自己的代码中做这样的事情（或类似的事情）。
#[derive(Debug)]
pub enum Error {
    ExecutableError(std::io::Error),
    OutputFormatError(&'static str),
}

// 为 Error 生成可读的表示形式
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self {
            Error::ExecutableError(err) => write!(f, "Error executing ps: {}", err),
            Error::OutputFormatError(err) => write!(f, "ps printed malformed output: {}", err),
        }
    }
}

// 使得可以自动将 std::io::Error 转换为我们的 Error 类型
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Error {
        Error::ExecutableError(error)
    }
}

// 使得可以自动将 std::string::FromUtf8Error 转换为我们的 Error 类型
impl From<std::string::FromUtf8Error> for Error {
    fn from(_error: std::string::FromUtf8Error) -> Error {
        Error::OutputFormatError("Output is not utf-8")
    }
}

// 使得可以自动将 std::string::ParseIntError 转换为我们的 Error 类型
impl From<std::num::ParseIntError> for Error {
    fn from(_error: std::num::ParseIntError) -> Error {
        Error::OutputFormatError("Error parsing integer")
    }
}

/// 这个函数接收一行用 -o "pid= ppid= command=" 格式化的 ps 输出，
/// 并返回一个从解析的输出初始化的 Process 结构体。
///
/// 示例行：
/// "  578   577 emacs inode.c"
fn parse_ps_line(line: &str) -> Result<Process, Error> {
    // ps 不会输出很好的机器可读输出，所以我们在这里做一些奇怪的事情来
    // 处理可变数量的空白字符。
    let mut remainder = line.trim();
    let first_token_end = remainder
        .find(char::is_whitespace)
        .ok_or(Error::OutputFormatError("Missing second column"))?;
    let pid = remainder[0..first_token_end].parse::<usize>()?;
    remainder = remainder[first_token_end..].trim_start();
    let second_token_end = remainder
        .find(char::is_whitespace)
        .ok_or(Error::OutputFormatError("Missing third column"))?;
    let ppid = remainder[0..second_token_end].parse::<usize>()?;
    remainder = remainder[second_token_end..].trim_start();
    Ok(Process::new(pid, ppid, String::from(remainder)))
}

/// 这个函数接收一个 pid 并返回指定进程的 Process 结构体，如果指定的 pid 不存在则返回 None。
/// 只有当 ps 无法执行或产生意外的输出格式时才会返回 Error。
fn get_process(pid: usize) -> Result<Option<Process>, Error> {
    // 运行 ps 来查找指定的 pid。我们使用 ? 运算符在执行 ps 失败或返回非 utf-8 输出时返回 Error。
    // (上面的额外 Error trait 用于自动将像 std::io::Error 或 std::string::FromUtf8Error 
    // 这样的错误转换为我们的自定义错误类型。)
    let output = String::from_utf8(
        Command::new("ps")
            .args(&["--pid", &pid.to_string(), "-o", "pid= ppid= command="])
            .output()?
            .stdout,
    )?;
    // 如果找到了进程并且输出解析成功，则返回 Some；如果 ps 没有产生输出（表示没有匹配的进程），
    // 则返回 None。注意使用 ? 来传播在解析输出时发生的错误。
    if output.trim().len() > 0 {
        Ok(Some(parse_ps_line(output.trim())?))
    } else {
        Ok(None)
    }
}

/// 这个函数接收一个 pid 并返回一个 Process 结构体列表，
/// 列表中包含所有以指定 pid 为父进程的进程。
/// 如果 ps 无法执行或产生意外的输出格式，则返回 Error。
pub fn get_child_processes(pid: usize) -> Result<Vec<Process>, Error> {
    let ps_output = Command::new("ps")
        .args(&["--ppid", &pid.to_string(), "-o", "pid= ppid= command="])
        .output()?;
    let mut output = Vec::new();
    for line in String::from_utf8(ps_output.stdout)?.lines() {
        output.push(parse_ps_line(line)?);
    }
    Ok(output)
}

/// 这个函数接收一个命令名（例如 "sort" 或 "./multi_pipe_test"）并返回第一个匹配进程的 pid，
/// 如果没有找到匹配的进程则返回 None。如果运行 pgrep 或解析 pgrep 的输出时出错，则返回 Error。
fn get_pid_by_command_name(name: &str) -> Result<Option<usize>, Error> {
    let output = String::from_utf8(
        Command::new("pgrep")
            .args(&["-xU", getuid().to_string().as_str(), name])
            .output()?
            .stdout,
    )?;
    Ok(match output.lines().next() {
        Some(line) => Some(line.parse::<usize>()?),
        None => None,
    })
}

/// 这个程序在系统上查找目标进程。指定的查询可以是命令名（例如 "./subprocess_test"）
/// 或 PID（例如 "5612"）。如果找到了指定的进程，此函数返回 Process 结构体；
/// 如果没有找到匹配的进程，返回 None；如果在运行 ps 或 pgrep 时遇到错误，返回 Error。
pub fn get_target(query: &str) -> Result<Option<Process>, Error> {
    let pid_by_command = get_pid_by_command_name(query)?;
    if pid_by_command.is_some() {
        return get_process(pid_by_command.unwrap());
    }
    // 如果将查询作为命令名搜索失败，让我们看看它是否是一个有效的 pid
    match query.parse() {
        Ok(pid) => return get_process(pid),
        Err(_) => return Ok(None),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::process::Child;

    fn start_c_program(program: &str) -> Child {
        Command::new(program)
            .spawn()
            .expect(&format!("Could not find {}. Have you run make?", program))
    }

    #[test]
    fn test_get_target_success() {
        let mut subprocess = start_c_program("./multi_pipe_test");
        let found = get_target("multi_pipe_test")
            .expect("Passed valid \"multi_pipe_test\" to get_target, but it returned an error")
            .expect("Passed valid \"multi_pipe_test\" to get_target, but it returned None");
        assert_eq!(found.command, "./multi_pipe_test");
        let _ = subprocess.kill();
    }

    #[test]
    fn test_get_target_invalid_command() {
        let found = get_target("asdflksadfasdf")
            .expect("get_target returned an error, even though ps and pgrep should be working");
        assert!(
            found.is_none(),
            "Passed invalid target to get_target, but it returned Some"
        );
    }

    #[test]
    fn test_get_target_invalid_pid() {
        let found = get_target("1234567890")
            .expect("get_target returned an error, even though ps and pgrep should be working");
        assert!(
            found.is_none(),
            "Passed invalid target to get_target, but it returned Some"
        );
    }
}
