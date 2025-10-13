use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::{fmt, fs};
use std::path::Path;

const O_WRONLY: usize = 00000001;  // 只写标志
const O_RDWR: usize = 00000002;    // 读写标志
const COLORS: [&str; 6] = [
    "\x1B[38;5;9m",
    "\x1B[38;5;10m",
    "\x1B[38;5;11m",
    "\x1B[38;5;12m",
    "\x1B[38;5;13m",
    "\x1B[38;5;14m",
];
const CLEAR_COLOR: &str = "\x1B[0m";  // 清除颜色

/// 这个枚举可以用来表示文件是只读、只写还是读写的。
/// 枚举本质上是一个值，可以是多种"事物"中的一种。
#[derive(Debug, Clone, PartialEq)]
pub enum AccessMode {
    Read,
    Write,
    ReadWrite,
}

impl fmt::Display for AccessMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Match 操作符在 Rust 中经常与枚举一起使用。它们的功能类似于
        // 其他语言中的 switch 语句（但可以更具表现力）。
        match self {
            AccessMode::Read => write!(f, "{}", "read"),
            AccessMode::Write => write!(f, "{}", "write"),
            AccessMode::ReadWrite => write!(f, "{}", "read/write"),
        }
    }
}

/// 存储系统上打开文件的信息。由于 Linux 内核实际上不会向用户空间暴露
/// 太多关于打开文件表的信息（cplayground 使用了修改过的内核），
/// 这个结构体包含了来自打开文件表和 vnode 表的信息。
#[derive(Debug, Clone, PartialEq)]
pub struct OpenFile {
    pub name: String,
    pub cursor: usize,
    pub access_mode: AccessMode,
}

impl OpenFile {
    pub fn new(name: String, cursor: usize, access_mode: AccessMode) -> OpenFile {
        OpenFile {
            name,
            cursor,
            access_mode,
        }
    }

    /// 这个函数接收打开文件的路径，并返回一个更易于人类理解的文件名称。
    ///
    /// * 对于普通文件，将简单地返回提供的路径。
    /// * 对于终端（以 /dev/pts 开头的文件），将返回 "<terminal>"。
    /// * 对于管道（格式为 pipe:[pipenum] 的文件名），将返回 "<pipe #pipenum>"。
    fn path_to_name(path: &str) -> String {
        if path.starts_with("/dev/pts/") {
            String::from("<terminal>")
        } else if path.starts_with("pipe:[") && path.ends_with("]") {
            let pipe_num = &path[path.find('[').unwrap() + 1..path.find(']').unwrap()];
            format!("<pipe #{}>", pipe_num)
        } else {
            String::from(path)
        }
    }

    /// 这个函数接收某个文件描述符的 /proc/{pid}/fdinfo/{fdnum} 文件内容，
    /// 并使用正则表达式提取该文件描述符的游标位置（从技术上讲，是 fd 指向的
    /// 打开文件表条目的位置）。如果在 fdinfo 文本中找不到游标，则返回 None。
    fn parse_cursor(fdinfo: &str) -> Option<usize> {
        // Regex::new 如果正则表达式有语法错误，将返回 Error。
        // 我们在这里调用 unwrap()，因为这表明我们的代码有明显的问题，
        // 但如果这是一个需要不崩溃的关键系统的代码，那么我们应该返回 Error。
        let re = Regex::new(r"pos:\s*(\d+)").unwrap();
        Some(
            re.captures(fdinfo)?
                .get(1)?
                .as_str()
                .parse::<usize>()
                .ok()?,
        )
    }

    /// 这个函数接收某个文件描述符的 /proc/{pid}/fdinfo/{fdnum} 文件内容，
    /// 并使用 fdinfo 文本中包含的 "flags:" 字段提取该打开文件的访问模式。
    /// 如果找不到 "flags" 字段，则返回 None。
    fn parse_access_mode(fdinfo: &str) -> Option<AccessMode> {
        // Regex::new 如果正则表达式有语法错误，将返回 Error。
        // 我们在这里调用 unwrap()，因为这表明我们的代码有明显的问题，
        // 但如果这是一个需要不崩溃的关键系统的代码，那么我们应该返回 Error。
        let re = Regex::new(r"flags:\s*(\d+)").unwrap();
        // 提取 flags 字段并将其解析为八进制
        let flags = usize::from_str_radix(re.captures(fdinfo)?.get(1)?.as_str(), 8).ok()?;
        if flags & O_WRONLY > 0 {
            Some(AccessMode::Write)
        } else if flags & O_RDWR > 0 {
            Some(AccessMode::ReadWrite)
        } else {
            Some(AccessMode::Read)
        }
    }

    /// 给定指定的进程和 fd 编号，此函数读取 /proc/{pid}/fd/{fdnum} 和
    /// /proc/{pid}/fdinfo/{fdnum} 来填充 OpenFile 结构体。如果 pid 或 fd
    /// 无效，或者必要的信息不可用，则返回 None。
    ///
    /// (注意：这个函数返回 Option 还是 Result 是风格和上下文的问题。
    /// 有些人可能会争辩说你应该返回 Result，这样你可以对可能出错的事情进行更细粒度的控制，
    /// 例如，你可能希望在进程没有指定的 fd 而失败时与读取 /proc 文件失败时进行不同的处理。
    /// 然而，这会显著增加错误处理的复杂性。在我们的情况下，这不需要是一个超级健壮的程序，
    /// 我们也不需要进行细粒度的错误处理，所以返回 Option 是一种简单的方式来表明
    /// "嘿，我们无法获取必要的信息"，而不必小题大做。)
    pub fn from_fd(pid: usize, fd: usize) -> Option<OpenFile> {
        // 读取 /proc/{pid}/fd/{fd} 符号链接以获取文件路径
        let path = format!("/proc/{}/fd/{}", pid, fd);
        let link = fs::read_link(path).ok()?;
        let name = OpenFile::path_to_name(&link.to_string_lossy());
        
        // 读取 /proc/{pid}/fdinfo/{fd} 文件以获取游标和访问模式信息
        let fdinfo_path = format!("/proc/{}/fdinfo/{}", pid, fd);
        let fdinfo_content = fs::read_to_string(fdinfo_path).ok()?;
        
        // 从 fdinfo 内容中解析游标位置
        let cursor = OpenFile::parse_cursor(&fdinfo_content)?;
        
        // 从 fdinfo 内容中解析访问模式
        let access_mode = OpenFile::parse_access_mode(&fdinfo_content)?;
        
        Some(OpenFile { 
            name, 
            cursor, 
            access_mode,
        })
    }

    /// 这个函数返回带有 ANSI 转义码的 OpenFile 名称，用于对管道名称进行着色。
    /// 它对管道名称进行哈希处理，使得相同的管道名称总是产生相同的颜色。
    /// 这对于使程序输出更易读很有用，因为用户可以快速看到指向特定管道的所有 fd。
    #[allow(unused)] // TODO: 在 Milestone 5 中删除这一行
    pub fn colorized_name(&self) -> String {
        if self.name.starts_with("<pipe") {
            let mut hash = DefaultHasher::new();
            self.name.hash(&mut hash);
            let hash_val = hash.finish();
            let color = COLORS[(hash_val % COLORS.len() as u64) as usize];
            format!("{}{}{}", color, self.name, CLEAR_COLOR)
        } else {
            format!("{}", self.name)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::ps_utils;
    use std::process::{Child, Command};

    fn start_c_program(program: &str) -> Child {
        Command::new(program)
            .spawn()
            .expect(&format!("Could not find {}. Have you run make?", program))
    }

    #[test]
    fn test_openfile_from_fd() {
        let mut test_subprocess = start_c_program("./multi_pipe_test");
        let process = ps_utils::get_target("multi_pipe_test").unwrap().unwrap();
        // 获取文件描述符 0，它应该指向终端
        let open_file = OpenFile::from_fd(process.pid, 0)
            .expect("Expected to get open file data for multi_pipe_test, but OpenFile::from_fd returned None");
        assert_eq!(open_file.name, "<terminal>");
        assert_eq!(open_file.cursor, 0);
        assert_eq!(open_file.access_mode, AccessMode::ReadWrite);
        let _ = test_subprocess.kill();
    }

    #[test]
    fn test_openfile_from_fd_invalid_fd() {
        let mut test_subprocess = start_c_program("./multi_pipe_test");
        let process = ps_utils::get_target("multi_pipe_test").unwrap().unwrap();
        // 获取文件描述符 30，它应该是无效的
        assert!(
            OpenFile::from_fd(process.pid, 30).is_none(),
            "Expected None because file descriptor 30 is invalid"
        );
        let _ = test_subprocess.kill();
    }
}
