//! A module for working with processes for the Glyim standard library.
//!
//! This module provides functions for spawning and interacting with child processes,
//! as well as exiting the current process.

/// The `Command` struct for building up process execution configurations.
struct Command {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    current_dir: Option<String>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
}

impl Command {
    /// Construct a new `Command` for launching the program at path `program`.
    fn new(program: &str) -> Command {
        Command {
            program: program.to_string(),
            args: Vec::new(),
            env: Vec::new(),
            current_dir: Option::None,
            stdin: Stdio::Inherit,
            stdout: Stdio::Inherit,
            stderr: Stdio::Inherit,
        }
    }

    /// Add an argument to pass to the program.
    fn arg(mut self, arg: &str) -> Command {
        self.args.push(arg.to_string());
        self
    }

    /// Add multiple arguments to pass to the program.
    fn args(mut self, args: &[&str]) -> Command {
        for arg in args {
            self.args.push(arg.to_string());
        }
        self
    }

    /// Insert or update an environment variable mapping.
    fn env(mut self, key: &str, value: &str) -> Command {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    /// Set the working directory for the child process.
    fn current_dir(mut self, dir: &str) -> Command {
        self.current_dir = Option::Some(dir.to_string());
        self
    }

    /// Set configuration for the child process's standard input.
    fn stdin(mut self, cfg: Stdio) -> Command {
        self.stdin = cfg;
        self
    }

    /// Set configuration for the child process's standard output.
    fn stdout(mut self, cfg: Stdio) -> Command {
        self.stdout = cfg;
        self
    }

    /// Set configuration for the child process's standard error.
    fn stderr(mut self, cfg: Stdio) -> Command {
        self.stderr = cfg;
        self
    }

    /// Execute the command as a child process, returning a handle to it.
    fn spawn(self) -> Result<Child, String> {
        extern "C" {
            fn glyim_process_spawn(
                program: *const u8,
                program_len: usize,
                args: *const u8,
                args_len: usize,
                stdin_cfg: u32,
                stdout_cfg: u32,
                stderr_cfg: u32,
            ) -> i32;
        }
        let args_str = self.args.join("\0");
        let stdin_cfg = self.stdin.to_raw();
        let stdout_cfg = self.stdout.to_raw();
        let stderr_cfg = self.stderr.to_raw();
        let pid = unsafe {
            glyim_process_spawn(
                self.program.as_ptr(),
                self.program.len(),
                args_str.as_ptr(),
                args_str.len(),
                stdin_cfg,
                stdout_cfg,
                stderr_cfg,
            )
        };
        if pid < 0 {
            Result::Err(format!("failed to spawn process: {}", self.program))
        } else {
            Result::Ok(Child {
                pid: pid as u32,
                stdin: Option::None,
                stdout: Option::None,
                stderr: Option::None,
            })
        }
    }

    /// Execute the command and wait for it to complete.
    fn status(self) -> Result<ExitStatus, String> {
        let child = self.spawn()?;
        child.wait()
    }

    /// Execute the command and collect its stdout.
    fn output(self) -> Result<Output, String> {
        let child = Command::new(&self.program)
            .args(self.args.as_slice())
            .stdout(Stdio::Piped)
            .stderr(Stdio::Piped)
            .spawn()?;
        child.wait_with_output()
    }
}

/// A process spawned by a `Command`.
struct Child {
    pid: u32,
    stdin: Option<ChildStdin>,
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
}

impl Child {
    /// Wait for the child to exit and return its exit status.
    fn wait(self) -> Result<ExitStatus, String> {
        extern "C" {
            fn glyim_process_wait(pid: u32) -> i32;
        }
        let status = unsafe { glyim_process_wait(self.pid) };
        if status < 0 {
            Result::Err(format!("failed to wait for process {}", self.pid))
        } else {
            Result::Ok(ExitStatus { code: status })
        }
    }

    /// Simultaneously wait for the child to exit and collect all remaining output on stdout/stderr.
    fn wait_with_output(self) -> Result<Output, String> {
        extern "C" {
            fn glyim_process_wait_output(
                pid: u32,
                stdout_buf: *mut u8,
                stdout_cap: usize,
                stderr_buf: *mut u8,
                stderr_cap: usize,
                stdout_len: *mut usize,
                stderr_len: *mut usize,
            ) -> i32;
        }
        let mut stdout_buf = Vec::with_capacity(65536);
        let mut stderr_buf = Vec::with_capacity(65536);
        let mut stdout_len = 0usize;
        let mut stderr_len = 0usize;
        let status = unsafe {
            glyim_process_wait_output(
                self.pid,
                stdout_buf.as_mut_ptr(),
                stdout_buf.capacity(),
                stderr_buf.as_mut_ptr(),
                stderr_buf.capacity(),
                &mut stdout_len,
                &mut stderr_len,
            )
        };
        if status < 0 {
            Result::Err(format!("failed to get output from process {}", self.pid))
        } else {
            unsafe {
                stdout_buf.set_len(stdout_len);
                stderr_buf.set_len(stderr_len);
            }
            Result::Ok(Output {
                status: ExitStatus { code: status },
                stdout: stdout_buf,
                stderr: stderr_buf,
            })
        }
    }

    /// Return the process ID.
    fn id(&self) -> u32 {
        self.pid
    }

    /// Forcefully kill the child process.
    fn kill(&self) -> Result<(), String> {
        extern "C" {
            fn glyim_process_kill(pid: u32) -> i32;
        }
        let rc = unsafe { glyim_process_kill(self.pid) };
        if rc < 0 {
            Result::Err(format!("failed to kill process {}", self.pid))
        } else {
            Result::Ok(())
        }
    }
}

/// A handle to a child process's standard input.
struct ChildStdin {
    fd: i32,
}

/// A handle to a child process's standard output.
struct ChildStdout {
    fd: i32,
}

/// A handle to a child process's standard error.
struct ChildStderr {
    fd: i32,
}

/// Describes what to do with a standard I/O stream for a child process.
enum Stdio {
    /// Inherit the parent's standard I/O stream.
    Inherit,
    /// Use a pipe for the standard I/O stream.
    Piped,
    /// Redirect the standard I/O stream to /dev/null (or NUL on Windows).
    Null,
}

impl Stdio {
    fn to_raw(&self) -> u32 {
        match self {
            Stdio::Inherit => 0,
            Stdio::Piped => 1,
            Stdio::Null => 2,
        }
    }
}

/// The exit status of a process.
struct ExitStatus {
    code: i32,
}

impl ExitStatus {
    /// Was termination successful? Signal termination is not considered a success.
    fn success(&self) -> bool {
        self.code == 0
    }

    /// Return the exit code, if any.
    fn code(&self) -> Option<i32> {
        Option::Some(self.code)
    }
}

/// The output of a finished process.
struct Output {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Output {
    /// Return the exit status.
    fn status(&self) -> &ExitStatus {
        &self.status
    }

    /// Return the stdout as bytes.
    fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// Return the stderr as bytes.
    fn stderr(&self) -> &[u8] {
        &self.stderr
    }
}

/// Terminate the current process with the specified exit code.
fn exit(code: i32) -> ! {
    extern "C" {
        fn glyim_process_exit(code: i32) -> !;
    }
    unsafe { glyim_process_exit(code) }
}

/// Get the ID of the current process.
fn id() -> u32 {
    extern "C" {
        fn glyim_process_getpid() -> u32;
    }
    unsafe { glyim_process_getpid() }
}

/// Get the ID of the parent process.
fn parent_id() -> u32 {
    extern "C" {
        fn glyim_process_getppid() -> u32;
    }
    unsafe { glyim_process_getppid() }
}

/// Abort the process abnormally.
fn abort() -> ! {
    extern "C" {
        fn glyim_process_abort() -> !;
    }
    unsafe { glyim_process_abort() }
}
