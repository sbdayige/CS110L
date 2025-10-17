use crate::debugger_command::DebuggerCommand;
use crate::inferior::{Inferior};
use crate::dwarf_data::{DwarfData, Error as DwarfError};
use rustyline::error::ReadlineError;
use rustyline::Editor;

pub struct Debugger {
    target: String,
    history_path: String,
    readline: Editor<()>,
    inferior: Option<Inferior>,
    debug_data: Option<DwarfData>,
    breakpoints: Vec<usize>,
}

fn parse_address(addr: &str) -> Option<usize> {
    let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
        &addr[2..]
    } else {
        &addr
    };
    usize::from_str_radix(addr_without_0x, 16).ok()
}

impl Debugger {
    /// Initializes the debugger.
    pub fn new(target: &str) -> Debugger {
        let debug_data = match DwarfData::from_file(target) {
            Ok(val) => val,
            Err(DwarfError::ErrorOpeningFile) => {
                println!("Could not open file {}", target);
                std::process::exit(1);
            }
            Err(DwarfError::DwarfFormatError(err)) => {
                println!("Could not debugging symbols from {}: {:?}", target, err);
                std::process::exit(1);
            }
        };
        
        // Print debug information at startup
        debug_data.print();
        
        let history_path = format!("{}/.deet_history", std::env::var("HOME").unwrap());
        let mut readline = Editor::<()>::new();
        // Attempt to load history from ~/.deet_history if it exists
        let _ = readline.load_history(&history_path);

        Debugger {
            target: target.to_string(),
            history_path,
            readline,
            inferior: None,
            debug_data: Some(debug_data),
            breakpoints: Vec::new(),
        }
    }

    pub fn run(&mut self) {
        loop {
            match self.get_next_command() {
                DebuggerCommand::Run(args) => {
                    // Kill any existing inferior process before starting a new one
                    if let Some(ref mut inferior) = self.inferior {
                        let _ = inferior.kill();
                    }
                    
                    if let Some(inferior) = Inferior::new(&self.target, &args,&self.breakpoints) {
                        // Create the inferior
                        self.inferior = Some(inferior);
                        
                        // Continue the inferior and print its status
                        match self.inferior.as_mut().unwrap().cont() {
                            Ok(status) => {
                                match status {
                                    crate::inferior::Status::Stopped(signal, rip) => {
                                        println!("Child stopped (signal {})", signal);
                                        if let Some(debug_data) = &self.debug_data {
                                            let line = match debug_data.get_line_from_addr(rip) {
                                                Some(line) => line,
                                                None => {
                                                    println!("Unknown function");
                                                    break;
                                                }
                                            };
                                            println!("Stopped at {}",line);
                                        }
                                    }
                                    crate::inferior::Status::Exited(exit_code) => {
                                        println!("Child exited (status {})", exit_code);
                                    }
                                    crate::inferior::Status::Signaled(signal) => {
                                        println!("Child terminated (signal {})", signal);
                                    }
                                }
                            }
                            Err(err) => {
                                println!("Error continuing inferior: {}", err);
                            }
                        }
                    } else {
                        println!("Error starting subprocess");
                    }
                }
                
                DebuggerCommand::Continue => {
                    // Check if there is an inferior process running
                    if let Some(ref mut inferior) = self.inferior {
                        // Continue the inferior and print its status
                        match inferior.cont() {
                            Ok(status) => {
                                match status {
                                    crate::inferior::Status::Stopped(signal, rip) => {
                                        println!("Child stopped (signal {})", signal);
                                        if let Some(debug_data) = &self.debug_data {
                                            if let Some(line) = debug_data.get_line_from_addr(rip) {
                                                println!("Stopped at {}", line);
                                            }
                                        }
                                    }
                                    crate::inferior::Status::Exited(exit_code) => {
                                        println!("Child exited (status {})", exit_code);
                                    }
                                    crate::inferior::Status::Signaled(signal) => {
                                        println!("Child terminated (signal {})", signal);
                                    }
                                }
                            }
                            Err(err) => {
                                println!("Error continuing inferior: {}", err);
                            }
                        }
                    } else {
                        println!("No inferior process running");
                    }
                }

               DebuggerCommand::Backtrace => {
                    // return ;
                    if let Some(inferior) = &self.inferior {
                        if let Some(debug_data) = & self.debug_data{
                            let _ = inferior.print_backtrace(debug_data);
                        }
                    }                
                }
                
                DebuggerCommand::Break(target) => {
                    let addr = if target.starts_with('*') {
                        // Raw address (starts with *)
                        let addr_str = &target[1..];
                        match parse_address(addr_str) {
                            Some(addr) => Some(addr),
                            None => {
                                println!("Invalid address format: {}", addr_str);
                                continue;
                            }
                        }
                    } else if let Ok(line_number) = target.parse::<usize>() {
                        // Line number
                        if let Some(debug_data) = &self.debug_data {
                            match debug_data.get_addr_for_line(None, line_number) {
                                Some(addr) => Some(addr),
                                None => {
                                    println!("No code found at line {}", line_number);
                                    continue;
                                }
                            }
                        } else {
                            println!("No debug information available");
                            continue;
                        }
                    } else {
                        // Function name
                        if let Some(debug_data) = &self.debug_data {
                            match debug_data.get_addr_for_function(None, &target) {
                                Some(addr) => Some(addr),
                                None => {
                                    println!("Function '{}' not found", target);
                                    continue;
                                }
                            }
                        } else {
                            println!("No debug information available");
                            continue;
                        }
                    };

                    if let Some(addr) = addr {
                        // Add to breakpoints list
                        self.breakpoints.push(addr);
                        let breakpoint_num = self.breakpoints.len() - 1;
                        println!("Set breakpoint {} at {:#x}", breakpoint_num, addr);
                        
                        // If there's a running inferior, install the breakpoint immediately
                        if let Some(ref mut inferior) = self.inferior {
                            match inferior.install_breakpoint(addr) {
                                Ok(orig_byte) => {
                                    println!("Installed breakpoint at {:#x} (original byte: {:#x})", addr, orig_byte);
                                }
                                Err(e) => {
                                    eprintln!("Failed to install breakpoint at {:#x}: {}", addr, e);
                                }
                            }
                        }
                    }
                }

                DebuggerCommand::Quit => {
                    // Kill any existing inferior process before quitting
                    if let Some(ref mut inferior) = self.inferior {
                        let _ = inferior.kill();
                    }
                    return;
                }
            }
        }
    }

    /// This function prompts the user to enter a command, and continues re-prompting until the user
    /// enters a valid command. It uses DebuggerCommand::from_tokens to do the command parsing.
    ///
    /// You don't need to read, understand, or modify this function.
    fn get_next_command(&mut self) -> DebuggerCommand {
        loop {
            // Print prompt and get next line of user input
            match self.readline.readline("(deet) ") {
                Err(ReadlineError::Interrupted) => {
                    // User pressed ctrl+c. We're going to ignore it
                    println!("Type \"quit\" to exit");
                }
                Err(ReadlineError::Eof) => {
                    // User pressed ctrl+d, which is the equivalent of "quit" for our purposes
                    return DebuggerCommand::Quit;
                }
                Err(err) => {
                    panic!("Unexpected I/O error: {:?}", err);
                }
                Ok(line) => {
                    if line.trim().len() == 0 {
                        continue;
                    }
                    self.readline.add_history_entry(line.as_str());
                    if let Err(err) = self.readline.save_history(&self.history_path) {
                        println!(
                            "Warning: failed to save history file at {}: {}",
                            self.history_path, err
                        );
                    }
                    let tokens: Vec<&str> = line.split_whitespace().collect();
                    if let Some(cmd) = DebuggerCommand::from_tokens(&tokens) {
                        return cmd;
                    } else {
                        println!("Unrecognized command.");
                    }
                }
            }
        }
    }
}
