use std::ptr;

#[test]
fn test_glyim_process_spawn_and_wait() {
    unsafe {
        let mut child_handle: usize = 0;
        let result = crate::glyim_process_spawn(
            "true".as_ptr(),
            "true".len(),
            ptr::null(),
            0,
            &mut child_handle,
        );
        assert_eq!(result, 0, "glyim_process_spawn should succeed for 'true'");
        assert_ne!(child_handle, 0, "child_handle should not be 0");

        let mut exit_code: i32 = -999;
        let wait_result = crate::glyim_process_wait(child_handle, &mut exit_code);
        assert_eq!(wait_result, 0, "glyim_process_wait should succeed");
        assert_eq!(exit_code, 0, "'true' should exit with code 0");
    }
}

#[test]
fn test_glyim_process_spawn_with_args() {
    unsafe {
        let args = "echo\0hello\0world";
        let mut child_handle: usize = 0;
        let result = crate::glyim_process_spawn(
            "echo".as_ptr(),
            "echo".len(),
            args.as_ptr(),
            args.len(),
            &mut child_handle,
        );
        assert_eq!(result, 0, "glyim_process_spawn should succeed for 'echo'");
        assert_ne!(child_handle, 0, "child_handle should not be 0");

        let mut exit_code: i32 = -999;
        let wait_result = crate::glyim_process_wait(child_handle, &mut exit_code);
        assert_eq!(wait_result, 0, "glyim_process_wait should succeed");
        assert_eq!(exit_code, 0, "'echo' should exit with code 0");
    }
}

#[test]
fn test_glyim_process_spawn_nonexistent() {
    unsafe {
        let mut child_handle: usize = 0;
        let result = crate::glyim_process_spawn(
            "glyim_nonexistent_binary_12345".as_ptr(),
            "glyim_nonexistent_binary_12345".len(),
            ptr::null(),
            0,
            &mut child_handle,
        );
        assert_eq!(
            result, -1,
            "glyim_process_spawn should fail for nonexistent binary"
        );
    }
}

#[test]
fn test_glyim_process_wait_output() {
    unsafe {
        let args = "echo\0test_output";
        let mut child_handle: usize = 0;
        let spawn_result = crate::glyim_process_spawn(
            "echo".as_ptr(),
            "echo".len(),
            args.as_ptr(),
            args.len(),
            &mut child_handle,
        );
        assert_eq!(spawn_result, 0, "spawn should succeed");

        let mut stdout_ptr: *mut u8 = ptr::null_mut();
        let mut stdout_len: usize = 0;
        let mut stderr_ptr: *mut u8 = ptr::null_mut();
        let mut stderr_len: usize = 0;
        let mut exit_code: i32 = -999;
        let wait_result = crate::glyim_process_wait_output(
            child_handle,
            &mut stdout_ptr,
            &mut stdout_len,
            &mut stderr_ptr,
            &mut stderr_len,
            &mut exit_code,
        );
        assert_eq!(wait_result, 0, "glyim_process_wait_output should succeed");
        assert_eq!(exit_code, 0, "exit code should be 0");
        assert!(!stdout_ptr.is_null(), "stdout_ptr should not be null");
        assert!(stdout_len > 0, "stdout_len should be > 0");

        let output = std::slice::from_raw_parts(stdout_ptr, stdout_len);
        let output_str = std::str::from_utf8(output).expect("output should be valid UTF-8");
        assert!(
            output_str.contains("test_output"),
            "output should contain 'test_output'"
        );

        crate::glyim_free_cstr(stdout_ptr);
        if !stderr_ptr.is_null() {
            crate::glyim_free_cstr(stderr_ptr);
        }
    }
}

#[cfg(unix)]
#[test]
fn test_glyim_process_spawn_preserves_spaces_in_arg() {
    // Plan §17.2: arguments must not be re-split on whitespace. A single
    // argument containing spaces must arrive at the child as ONE argv element.
    // `printf` with format "%s\n" and a space-containing value prints the
    // value verbatim; if the space arg were ever split, `printf` would receive
    // the value as two separate args and print "with\nspace" instead.
    unsafe {
        // args = "%s\n" NUL "with space"
        let args = "%s\n\0with space";
        let mut child_handle: usize = 0;
        let result = crate::glyim_process_spawn(
            "printf".as_ptr(),
            "printf".len(),
            args.as_ptr(),
            args.len(),
            &mut child_handle,
        );
        assert_eq!(result, 0, "glyim_process_spawn should succeed for 'printf'");
        assert_ne!(child_handle, 0, "child_handle should not be 0");

        let mut stdout_ptr: *mut u8 = ptr::null_mut();
        let mut stdout_len: usize = 0;
        let mut stderr_ptr: *mut u8 = ptr::null_mut();
        let mut stderr_len: usize = 0;
        let mut exit_code: i32 = -999;
        let wait_result = crate::glyim_process_wait_output(
            child_handle,
            &mut stdout_ptr,
            &mut stdout_len,
            &mut stderr_ptr,
            &mut stderr_len,
            &mut exit_code,
        );
        assert_eq!(wait_result, 0, "glyim_process_wait_output should succeed");
        assert_eq!(exit_code, 0, "exit code should be 0");
        assert!(!stdout_ptr.is_null(), "stdout_ptr should not be null");

        let output = std::slice::from_raw_parts(stdout_ptr, stdout_len);
        let output_str = std::str::from_utf8(output).expect("output should be valid UTF-8");
        assert_eq!(
            output_str, "with space\n",
            "a space-containing argument must be passed as a single argv element"
        );

        crate::glyim_free_cstr(stdout_ptr);
        if !stderr_ptr.is_null() {
            crate::glyim_free_cstr(stderr_ptr);
        }
    }
}

#[test]
fn test_glyim_process_getpid() {
    let pid = crate::glyim_process_getpid();
    assert!(pid > 0, "PID should be positive");
}

#[cfg(unix)]
#[test]
fn test_glyim_process_getppid() {
    let ppid = crate::glyim_process_getppid();
    assert!(ppid > 0, "PPID should be positive");
}

#[test]
fn test_glyim_process_kill_invalid_handle() {
    unsafe {
        let result = crate::glyim_process_kill(999_999, 9);
        assert_eq!(result, -1, "should fail for invalid handle");
    }
}

#[test]
fn test_glyim_process_kill_honors_signal() {
    // Spawn a long-running child; send SIGTERM (15) and confirm the kill
    // succeeds and the child is reaped with a signal-terminated status.
    unsafe {
        let mut handle: usize = 0;
        let spawn = crate::glyim_process_spawn(
            c"sleep".as_ptr().cast::<u8>(),
            5,
            std::ptr::null(),
            0,
            &mut handle as *mut usize,
        );
        assert_eq!(spawn, 0, "spawn of 'sleep' should succeed");
        assert_ne!(handle, 0, "handle must be assigned");

        // SIGTERM (15) is honored, not forced to SIGKILL.
        let kill = crate::glyim_process_kill(handle, 15);
        assert_eq!(kill, 0, "kill with SIGTERM should succeed");

        let mut code: i32 = -999;
        let wait = crate::glyim_process_wait(handle, &mut code as *mut i32);
        assert_eq!(wait, 0, "wait after kill should reap the child");
        // A signal-terminated child reports a non-zero (negative/signal) code.
        assert_ne!(code, 0, "terminated child should not report success");
    }
}

#[cfg(windows)]
#[test]
fn test_glyim_process_kill_graceful_signal_windows() {
    // Spawn a long-running child in its own process group, then send SIGTERM
    // (15) and assert the new Windows graceful path returns success and the
    // child is eventually reaped. `ping -n 20 127.0.0.1` runs ~20s on Windows
    // and is a reliable long-running stand-in (no `sleep` on cmd.exe).
    unsafe {
        let mut handle: usize = 0;
        let spawn = crate::glyim_process_spawn(
            b"ping\x00".as_ptr(),
            4,
            b"-n\x0020\x00127.0.0.1\x00".as_ptr(),
            19,
            &mut handle as *mut usize,
        );
        assert_eq!(spawn, 0, "spawn of 'ping' should succeed");
        assert_ne!(handle, 0, "handle must be assigned");

        // SIGTERM (15) must take the graceful GenerateConsoleCtrlEvent path
        // (possibly falling back to a hard kill) and report success.
        let kill = crate::glyim_process_kill(handle, 15);
        assert_eq!(kill, 0, "graceful kill on Windows should succeed");

        let mut code: i32 = -999;
        let wait = crate::glyim_process_wait(handle, &mut code as *mut i32);
        assert_eq!(wait, 0, "wait after kill should reap the child");
    }
}

#[cfg(windows)]
#[test]
fn test_glyim_process_kill_hard_signal_windows() {
    // SIGKILL (9) must take the hard TerminateProcess path on Windows.
    unsafe {
        let mut handle: usize = 0;
        let spawn = crate::glyim_process_spawn(
            b"ping\x00".as_ptr(),
            4,
            b"-n\x0020\x00127.0.0.1\x00".as_ptr(),
            19,
            &mut handle as *mut usize,
        );
        assert_eq!(spawn, 0);
        let kill = crate::glyim_process_kill(handle, 9);
        assert_eq!(kill, 0, "hard kill on Windows should succeed");
        let mut code: i32 = -999;
        let wait = crate::glyim_process_wait(handle, &mut code as *mut i32);
        assert_eq!(wait, 0);
    }
}
