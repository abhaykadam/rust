// Copyright 2012-2013 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Utilities for spawning and managing processes

#[allow(missing_doc)];

use comm::SharedChan;
use io::Reader;
use io::process::ProcessExit;
use io::process;
use io;
use libc::{pid_t, c_int};
use libc;
use prelude::*;

/**
 * A value representing a child process.
 *
 * The lifetime of this value is linked to the lifetime of the actual
 * process - the Process destructor calls self.finish() which waits
 * for the process to terminate.
 */
pub struct Process {
    priv inner: process::Process,
}

/// Options that can be given when starting a Process.
pub struct ProcessOptions<'a> {
    /**
     * If this is None then the new process will have the same initial
     * environment as the parent process.
     *
     * If this is Some(vec-of-names-and-values) then the new process will
     * have an environment containing the given named values only.
     */
    env: Option<~[(~str, ~str)]>,

    /**
     * If this is None then the new process will use the same initial working
     * directory as the parent process.
     *
     * If this is Some(path) then the new process will use the given path
     * for its initial working directory.
     */
    dir: Option<&'a Path>,

    /**
     * If this is None then a new pipe will be created for the new process's
     * input and Process.input() will provide a Writer to write to this pipe.
     *
     * If this is Some(file-descriptor) then the new process will read its input
     * from the given file descriptor, Process.input_redirected() will return
     * true, and Process.input() will fail.
     */
    in_fd: Option<c_int>,

    /**
     * If this is None then a new pipe will be created for the new program's
     * output and Process.output() will provide a Reader to read from this pipe.
     *
     * If this is Some(file-descriptor) then the new process will write its output
     * to the given file descriptor, Process.output_redirected() will return
     * true, and Process.output() will fail.
     */
    out_fd: Option<c_int>,

    /**
     * If this is None then a new pipe will be created for the new program's
     * error stream and Process.error() will provide a Reader to read from this pipe.
     *
     * If this is Some(file-descriptor) then the new process will write its error output
     * to the given file descriptor, Process.error_redirected() will return true, and
     * and Process.error() will fail.
     */
    err_fd: Option<c_int>,
}

impl <'a> ProcessOptions<'a> {
    /// Return a ProcessOptions that has None in every field.
    pub fn new<'a>() -> ProcessOptions<'a> {
        ProcessOptions {
            env: None,
            dir: None,
            in_fd: None,
            out_fd: None,
            err_fd: None,
        }
    }
}

/// The output of a finished process.
pub struct ProcessOutput {
    /// The status (exit code) of the process.
    status: ProcessExit,

    /// The data that the process wrote to stdout.
    output: ~[u8],

    /// The data that the process wrote to stderr.
    error: ~[u8],
}

impl Process {
    /**
     * Spawns a new Process.
     *
     * # Arguments
     *
     * * prog - The path to an executable.
     * * args - Vector of arguments to pass to the child process.
     * * options - Options to configure the environment of the process,
     *             the working directory and the standard IO streams.
     */
    pub fn new(prog: &str, args: &[~str], options: ProcessOptions) -> Option<Process> {
        let ProcessOptions { env, dir, in_fd, out_fd, err_fd } = options;
        let env = env.as_ref().map(|a| a.as_slice());
        let cwd = dir.as_ref().map(|a| a.as_str().unwrap());
        fn rtify(fd: Option<c_int>, input: bool) -> process::StdioContainer {
            match fd {
                Some(fd) => process::InheritFd(fd),
                None => process::CreatePipe(input, !input),
            }
        }
        let rtio = [rtify(in_fd, true), rtify(out_fd, false),
                    rtify(err_fd, false)];
        let rtconfig = process::ProcessConfig {
            program: prog,
            args: args,
            env: env,
            cwd: cwd,
            io: rtio,
        };
        match process::Process::new(rtconfig) {
            Some(inner) => Some(Process { inner: inner }),
            None => None
        }
    }

    /// Returns the unique id of the process
    pub fn get_id(&self) -> pid_t { self.inner.id() }

    /**
     * Returns an io::Writer that can be used to write to this Process's stdin.
     *
     * Fails if there is no stdin available (it's already been removed by
     * take_input)
     */
    pub fn input<'a>(&'a mut self) -> &'a mut io::Writer {
        self.inner.io[0].get_mut_ref() as &mut io::Writer
    }

    /**
     * Returns an io::Reader that can be used to read from this Process's stdout.
     *
     * Fails if there is no stdout available (it's already been removed by
     * take_output)
     */
    pub fn output<'a>(&'a mut self) -> &'a mut io::Reader {
        self.inner.io[1].get_mut_ref() as &mut io::Reader
    }

    /**
     * Returns an io::Reader that can be used to read from this Process's stderr.
     *
     * Fails if there is no stderr available (it's already been removed by
     * take_error)
     */
    pub fn error<'a>(&'a mut self) -> &'a mut io::Reader {
        self.inner.io[2].get_mut_ref() as &mut io::Reader
    }

    /**
     * Closes the handle to the child process's stdin.
     */
    pub fn close_input(&mut self) {
        self.inner.io[0].take();
    }

    /**
     * Closes the handle to stdout and stderr.
     */
    pub fn close_outputs(&mut self) {
        self.inner.io[1].take();
        self.inner.io[2].take();
    }

    /**
     * Closes the handle to stdin, waits for the child process to terminate,
     * and returns the exit code.
     *
     * If the child has already been finished then the exit code is returned.
     */
    pub fn finish(&mut self) -> ProcessExit { self.inner.wait() }

    /**
     * Closes the handle to stdin, waits for the child process to terminate, and
     * reads and returns all remaining output of stdout and stderr, along with
     * the exit code.
     *
     * If the child has already been finished then the exit code and any
     * remaining unread output of stdout and stderr will be returned.
     *
     * This method will fail if the child process's stdout or stderr streams
     * were redirected to existing file descriptors.
     */
    pub fn finish_with_output(&mut self) -> ProcessOutput {
        self.close_input();
        let output = self.inner.io[1].take();
        let error = self.inner.io[2].take();

        // Spawn two entire schedulers to read both stdout and sterr
        // in parallel so we don't deadlock while blocking on one
        // or the other. FIXME (#2625): Surely there's a much more
        // clever way to do this.
        let (p, ch) = SharedChan::new();
        let ch_clone = ch.clone();

        do spawn {
            let _guard = io::ignore_io_error();
            let mut error = error;
            match error {
                Some(ref mut e) => ch.send((2, e.read_to_end())),
                None => ch.send((2, ~[]))
            }
        }
        do spawn {
            let _guard = io::ignore_io_error();
            let mut output = output;
            match output {
                Some(ref mut e) => ch_clone.send((1, e.read_to_end())),
                None => ch_clone.send((1, ~[]))
            }
        }

        let status = self.finish();

        let (errs, outs) = match (p.recv(), p.recv()) {
            ((1, o), (2, e)) => (e, o),
            ((2, e), (1, o)) => (e, o),
            ((x, _), (y, _)) => {
                fail!("unexpected file numbers: {}, {}", x, y);
            }
        };

        return ProcessOutput {status: status,
                              output: outs,
                              error: errs};
    }

    /**
     * Terminates the process, giving it a chance to clean itself up if
     * this is supported by the operating system.
     *
     * On Posix OSs SIGTERM will be sent to the process. On Win32
     * TerminateProcess(..) will be called.
     */
    pub fn destroy(&mut self) {
        self.inner.signal(io::process::PleaseExitSignal);
        self.finish();
    }

    /**
     * Terminates the process as soon as possible without giving it a
     * chance to clean itself up.
     *
     * On Posix OSs SIGKILL will be sent to the process. On Win32
     * TerminateProcess(..) will be called.
     */
    pub fn force_destroy(&mut self) {
        self.inner.signal(io::process::MustDieSignal);
        self.finish();
    }
}

/**
 * Spawns a process and waits for it to terminate. The process will
 * inherit the current stdin/stdout/stderr file descriptors.
 *
 * # Arguments
 *
 * * prog - The path to an executable
 * * args - Vector of arguments to pass to the child process
 *
 * # Return value
 *
 * The process's exit code, or None if the child process could not be started
 */
pub fn process_status(prog: &str, args: &[~str]) -> Option<ProcessExit> {
    let mut opt_prog = Process::new(prog, args, ProcessOptions {
        env: None,
        dir: None,
        in_fd: Some(unsafe { libc::dup(libc::STDIN_FILENO) }),
        out_fd: Some(unsafe { libc::dup(libc::STDOUT_FILENO) }),
        err_fd: Some(unsafe { libc::dup(libc::STDERR_FILENO) })
    });
    match opt_prog {
        Some(ref mut prog) => Some(prog.finish()),
        None => None
    }
}

/**
 * Spawns a process, records all its output, and waits for it to terminate.
 *
 * # Arguments
 *
 * * prog - The path to an executable
 * * args - Vector of arguments to pass to the child process
 *
 * # Return value
 *
 * The process's stdout/stderr output and exit code, or None if the child process could not be
 * started.
 */
pub fn process_output(prog: &str, args: &[~str]) -> Option<ProcessOutput> {
    let mut opt_prog = Process::new(prog, args, ProcessOptions::new());
    match opt_prog {
        Some(ref mut prog) => Some(prog.finish_with_output()),
        None => None
    }
}

#[cfg(test)]
mod tests {
    use prelude::*;
    use os;
    use run;
    use str;
    use task::spawn;
    use unstable::running_on_valgrind;
    use io::pipe::PipeStream;
    use io::{io_error, FileNotFound};
    use libc::c_int;

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_process_status() {
        let mut status = run::process_status("false", []).expect("failed to exec `false`");
        assert!(status.matches_exit_status(1));

        status = run::process_status("true", []).expect("failed to exec `true`");
        assert!(status.success());
    }

    #[test]
    fn test_process_output_fail_to_start() {
        // If the executable does not exist, then the io_error condition should be raised with
        // IoErrorKind FileNotFound.

        let mut trapped_io_error = false;
        let opt_outp = io_error::cond.trap(|e| {
            trapped_io_error = true;
            assert_eq!(e.kind, FileNotFound);
        }).inside(|| -> Option<run::ProcessOutput> {
            run::process_output("no-binary-by-this-name-should-exist", [])
        });
        assert!(trapped_io_error);
        assert!(opt_outp.is_none());
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_process_output_output() {

        let run::ProcessOutput {status, output, error}
             = run::process_output("echo", [~"hello"]).expect("failed to exec `echo`");
        let output_str = str::from_utf8_owned(output).unwrap();

        assert!(status.success());
        assert_eq!(output_str.trim().to_owned(), ~"hello");
        // FIXME #7224
        if !running_on_valgrind() {
            assert_eq!(error, ~[]);
        }
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_process_output_error() {

        let run::ProcessOutput {status, output, error}
             = run::process_output("mkdir", [~"."]).expect("failed to exec `mkdir`");

        assert!(status.matches_exit_status(1));
        assert_eq!(output, ~[]);
        assert!(!error.is_empty());
    }

    #[test]
    #[ignore] // FIXME(#10016) cat never sees stdin close
    fn test_pipes() {

        let pipe_in = os::pipe();
        let pipe_out = os::pipe();
        let pipe_err = os::pipe();

        let mut process = run::Process::new("cat", [], run::ProcessOptions {
            dir: None,
            env: None,
            in_fd: Some(pipe_in.input),
            out_fd: Some(pipe_out.out),
            err_fd: Some(pipe_err.out)
        }).expect("failed to exec `cat`");

        os::close(pipe_in.input as int);
        os::close(pipe_out.out as int);
        os::close(pipe_err.out as int);

        do spawn {
            writeclose(pipe_in.out, "test");
        }
        let actual = readclose(pipe_out.input);
        readclose(pipe_err.input);
        process.finish();

        assert_eq!(~"test", actual);
    }

    fn writeclose(fd: c_int, s: &str) {
        let mut writer = PipeStream::open(fd);
        writer.write(s.as_bytes());
    }

    fn readclose(fd: c_int) -> ~str {
        let mut res = ~[];
        let mut reader = PipeStream::open(fd);
        let mut buf = [0, ..1024];
        loop {
            match reader.read(buf) {
                Some(n) => { res.push_all(buf.slice_to(n)); }
                None => break
            }
        }
        str::from_utf8_owned(res).unwrap()
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_finish_once() {
        let mut prog = run::Process::new("false", [], run::ProcessOptions::new())
            .expect("failed to exec `false`");
        assert!(prog.finish().matches_exit_status(1));
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_finish_twice() {
        let mut prog = run::Process::new("false", [], run::ProcessOptions::new())
            .expect("failed to exec `false`");
        assert!(prog.finish().matches_exit_status(1));
        assert!(prog.finish().matches_exit_status(1));
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_finish_with_output_once() {

        let mut prog = run::Process::new("echo", [~"hello"], run::ProcessOptions::new())
            .expect("failed to exec `echo`");
        let run::ProcessOutput {status, output, error}
            = prog.finish_with_output();
        let output_str = str::from_utf8_owned(output).unwrap();

        assert!(status.success());
        assert_eq!(output_str.trim().to_owned(), ~"hello");
        // FIXME #7224
        if !running_on_valgrind() {
            assert_eq!(error, ~[]);
        }
    }

    #[test]
    #[cfg(not(target_os="android"))] // FIXME(#10380)
    fn test_finish_with_output_twice() {

        let mut prog = run::Process::new("echo", [~"hello"], run::ProcessOptions::new())
            .expect("failed to exec `echo`");
        let run::ProcessOutput {status, output, error}
            = prog.finish_with_output();

        let output_str = str::from_utf8_owned(output).unwrap();

        assert!(status.success());
        assert_eq!(output_str.trim().to_owned(), ~"hello");
        // FIXME #7224
        if !running_on_valgrind() {
            assert_eq!(error, ~[]);
        }

        let run::ProcessOutput {status, output, error}
            = prog.finish_with_output();

        assert!(status.success());
        assert_eq!(output, ~[]);
        // FIXME #7224
        if !running_on_valgrind() {
            assert_eq!(error, ~[]);
        }
    }

    #[cfg(unix,not(target_os="android"))]
    fn run_pwd(dir: Option<&Path>) -> run::Process {
        run::Process::new("pwd", [], run::ProcessOptions {
            dir: dir,
            .. run::ProcessOptions::new()
        }).expect("failed to exec `pwd`")
    }
    #[cfg(unix,target_os="android")]
    fn run_pwd(dir: Option<&Path>) -> run::Process {
        run::Process::new("/system/bin/sh", [~"-c",~"pwd"], run::ProcessOptions {
            dir: dir,
            .. run::ProcessOptions::new()
        }).expect("failed to exec `/system/bin/sh`")
    }

    #[cfg(windows)]
    fn run_pwd(dir: Option<&Path>) -> run::Process {
        run::Process::new("cmd", [~"/c", ~"cd"], run::ProcessOptions {
            dir: dir,
            .. run::ProcessOptions::new()
        }).expect("failed to run `cmd`")
    }

    #[test]
    fn test_keep_current_working_dir() {
        let mut prog = run_pwd(None);

        let output = str::from_utf8_owned(prog.finish_with_output().output).unwrap();
        let parent_dir = os::getcwd();
        let child_dir = Path::new(output.trim());

        let parent_stat = parent_dir.stat();
        let child_stat = child_dir.stat();

        assert_eq!(parent_stat.unstable.device, child_stat.unstable.device);
        assert_eq!(parent_stat.unstable.inode, child_stat.unstable.inode);
    }

    #[test]
    fn test_change_working_directory() {
        // test changing to the parent of os::getcwd() because we know
        // the path exists (and os::getcwd() is not expected to be root)
        let parent_dir = os::getcwd().dir_path();
        let mut prog = run_pwd(Some(&parent_dir));

        let output = str::from_utf8_owned(prog.finish_with_output().output).unwrap();
        let child_dir = Path::new(output.trim());

        let parent_stat = parent_dir.stat();
        let child_stat = child_dir.stat();

        assert_eq!(parent_stat.unstable.device, child_stat.unstable.device);
        assert_eq!(parent_stat.unstable.inode, child_stat.unstable.inode);
    }

    #[cfg(unix,not(target_os="android"))]
    fn run_env(env: Option<~[(~str, ~str)]>) -> run::Process {
        run::Process::new("env", [], run::ProcessOptions {
            env: env,
            .. run::ProcessOptions::new()
        }).expect("failed to exec `env`")
    }
    #[cfg(unix,target_os="android")]
    fn run_env(env: Option<~[(~str, ~str)]>) -> run::Process {
        run::Process::new("/system/bin/sh", [~"-c",~"set"], run::ProcessOptions {
            env: env,
            .. run::ProcessOptions::new()
        }).expect("failed to exec `/system/bin/sh`")
    }

    #[cfg(windows)]
    fn run_env(env: Option<~[(~str, ~str)]>) -> run::Process {
        run::Process::new("cmd", [~"/c", ~"set"], run::ProcessOptions {
            env: env,
            .. run::ProcessOptions::new()
        }).expect("failed to run `cmd`")
    }

    #[test]
    #[cfg(not(target_os="android"))]
    fn test_inherit_env() {
        if running_on_valgrind() { return; }

        let mut prog = run_env(None);
        let output = str::from_utf8_owned(prog.finish_with_output().output).unwrap();

        let r = os::env();
        for &(ref k, ref v) in r.iter() {
            // don't check windows magical empty-named variables
            assert!(k.is_empty() || output.contains(format!("{}={}", *k, *v)));
        }
    }
    #[test]
    #[cfg(target_os="android")]
    fn test_inherit_env() {
        if running_on_valgrind() { return; }

        let mut prog = run_env(None);
        let output = str::from_utf8_owned(prog.finish_with_output().output).unwrap();

        let r = os::env();
        for &(ref k, ref v) in r.iter() {
            // don't check android RANDOM variables
            if *k != ~"RANDOM" {
                assert!(output.contains(format!("{}={}", *k, *v)) ||
                        output.contains(format!("{}=\'{}\'", *k, *v)));
            }
        }
    }

    #[test]
    fn test_add_to_env() {

        let mut new_env = os::env();
        new_env.push((~"RUN_TEST_NEW_ENV", ~"123"));

        let mut prog = run_env(Some(new_env));
        let output = str::from_utf8_owned(prog.finish_with_output().output).unwrap();

        assert!(output.contains("RUN_TEST_NEW_ENV=123"));
    }
}
