use std::env;
use std::fs::File; // For read_file_lines()
use std::io::{self, BufRead}; // For read_file_lines()
use std::process;

fn read_file_lines(filename: &String) -> Result<Vec<String>, io::Error> {
    let file = File::open(filename)?;
    let mut l = Vec::new();
    for line in io::BufReader::new(file).lines() {
        let line_str = line?;
        l.push(line_str);
    }
    Ok(l)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Too few arguments.");
        process::exit(1);
    }
    let filename = &args[1];
    let lines = read_file_lines(filename).unwrap();

    let mut line_count = 0;
    let mut word_count = 0;
    let mut char_count = 0;

    for line in lines {
        line_count += 1;
        // 统计字符数（包括换行符）
        char_count += line.len() + 1; // +1 for newline character
                                      // 统计字数（按空白字符分割）
        word_count += line.split_whitespace().count();
    }

    // 输出统计结果，格式类似 wc 命令：行数 字数 字符数 文件名
    println!("{} {} {} {}", line_count, word_count, char_count, filename);
}
