use nix::sys::ptrace;
use nix::sys::signal;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::mem::size_of;
use std::process::Child;
use std::process::Command;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

use crate::dwarf_data::DwarfData;

fn align_addr_to_word(addr: usize) -> usize {
    addr & (-(size_of::<usize>() as isize) as usize)
}

#[derive(Debug)]
pub enum Status {
    /// Indicates inferior stopped. Contains the signal that stopped the process, as well as the
    /// current instruction pointer that it is stopped at.
    Stopped(signal::Signal, usize),

    /// Indicates inferior exited normally. Contains the exit status code.
    Exited(i32),

    /// Indicates the inferior exited due to a signal. Contains the signal that killed the
    /// process.
    Signaled(signal::Signal),
}

/// This function calls ptrace with PTRACE_TRACEME to enable debugging on a process. You should use
/// pre_exec with Command to call this in the child process.
fn child_traceme() -> Result<(), std::io::Error> {
    ptrace::traceme().or(Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "ptrace TRACEME failed",
    )))
}

pub struct Inferior {
    child: Child,
}

impl Inferior {
    /// Writes a single byte to the inferior's memory at the specified address.
    /// Returns the original byte that was at that address.
    fn write_byte(&mut self, addr: usize, val: u8) -> Result<u8, nix::Error> {
        let aligned_addr = align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((val as u64) << 8 * byte_offset);
        ptrace::write(
            self.pid(),
            aligned_addr as ptrace::AddressType,
            updated_word as *mut std::ffi::c_void,
        )?;
        Ok(orig_byte as u8)
    }

    /// Installs a breakpoint at the specified address by writing 0xcc to that location.
    /// Returns the original byte at that address, or an error if it fails.
    pub fn install_breakpoint(&mut self, addr: usize) -> Result<u8, nix::Error> {
        self.write_byte(addr, 0xcc)
    }

    /// Attempts to start a new inferior process. Returns Some(Inferior) if successful, or None if
    /// an error is encountered.
    pub fn new(target: &str, args: &Vec<String>, breakpoints: &Vec<usize>) -> Option<Inferior> {
        // Create a new command to execute the target program
        let mut command = Command::new(target);
        command.args(args);
        
        // Use pre_exec to call ptrace TRACEME in the child process before exec
        #[cfg(unix)]
        unsafe {
            command.pre_exec(child_traceme);
        }
        
        // Spawn the child process
        let child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                eprintln!("Error spawning child process: {}", e);
                return None;
            }
        };
        
        // Create the Inferior object
        let mut inferior = Inferior { child };
        
        // Wait for the child to stop (it will stop immediately after exec due to PTRACE_TRACEME)
        // We expect it to stop with SIGTRAP signal
        match inferior.wait(None) {
            Ok(Status::Stopped(signal::Signal::SIGTRAP, _)) => {
                // Install breakpoints after the inferior has fully loaded
                for &addr in breakpoints {
                    match inferior.write_byte(addr, 0xcc) {
                        Ok(orig_byte) => {
                            println!("Set breakpoint at {:#x} (original byte: {:#x})", addr, orig_byte);
                        }
                        Err(e) => {
                            eprintln!("Failed to set breakpoint at {:#x}: {}", addr, e);
                        }
                    }
                }
                Some(inferior)
            }
            Ok(status) => {
                eprintln!("Unexpected status while waiting for child: {:?}", status);
                None
            }
            Err(e) => {
                eprintln!("Error waiting for child: {}", e);
                None
            }
        }
    }

    /// Returns the pid of this inferior.
    pub fn pid(&self) -> Pid {
        nix::unistd::Pid::from_raw(self.child.id() as i32)
    }

    /// Calls waitpid on this inferior and returns a Status to indicate the state of the process
    /// after the waitpid call.
    pub fn wait(&self, options: Option<WaitPidFlag>) -> Result<Status, nix::Error> {
        Ok(match waitpid(self.pid(), options)? {
            WaitStatus::Exited(_pid, exit_code) => Status::Exited(exit_code),
            WaitStatus::Signaled(_pid, signal, _core_dumped) => Status::Signaled(signal),
            WaitStatus::Stopped(_pid, signal) => {
                let regs = ptrace::getregs(self.pid())?;
                Status::Stopped(signal, regs.rip as usize)
            }
            other => panic!("waitpid returned unexpected status: {:?}", other),
        })
    }

    /// Continues execution of the inferior process and waits until it stops or terminates.
    /// Returns the status of the inferior after it stops.
    pub fn cont(&self) -> Result<Status, nix::Error> {
        // Use ptrace::cont to wake up the inferior (pass None for sig)
        ptrace::cont(self.pid(), None)?;
        // Wait for the inferior to stop or terminate
        self.wait(None)
    }

    pub fn kill(&mut self) -> Result<(), std::io::Error> {
        println!("Killing running inferior (pid {})", self.pid());
        self.child.kill()
    }

    pub fn print_backtrace(&self, debug_data: &DwarfData) -> Result<(), nix::Error> {
        // Get the register values using ptrace::getregs
        let regs = ptrace::getregs(self.pid())?;
        let mut rip = regs.rip as usize;
        let mut rbp = regs.rbp as usize;
        
        loop {
            // Get the function name from the current instruction address
            let function_name = match debug_data.get_function_from_addr(rip) {
                Some(name) => name,
                None => {
                    println!("Unknown function at {:#x}", rip);
                    break;
                }
            };
            
            // Get the source file and line number from the current instruction address
            let line = match debug_data.get_line_from_addr(rip) {
                Some(line) => line,
                None => {
                    println!("Unknown location for function {}", function_name);
                    break;
                }
            };
            
            // Print the backtrace information
            println!("{} ({}:{})", function_name, line.file, line.number);
            
            // Check if we've reached the main function
            if function_name == "main" {
                break;
            }
    
            // Read the return address (saved rip) from [rbp + 8]
            rip = ptrace::read(self.pid(), (rbp + 8) as ptrace::AddressType)? as usize;
            
            // Read the saved frame pointer (previous rbp) from [rbp]
            rbp = ptrace::read(self.pid(), rbp as ptrace::AddressType)? as usize;
            
            // Safety check: if rbp is 0 or rip is 0, we've reached the end of the stack
            if rbp == 0 || rip == 0 {
                break;
            }
        }
        
        Ok(())
    }
}
