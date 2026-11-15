//! Bounded, time-limited Windows process execution.
//!
//! Each child is assigned to a Job Object with KILL_ON_JOB_CLOSE so descendants
//! inherit the job and are terminated when the command times out or NOVA drops
//! the job handle. Output is drained continuously but retained only up to a
//! fixed cap per stream.

use std::io::{self, Read};
use std::process::ExitStatus;
#[cfg(windows)]
use std::process::{Child, Command, Stdio};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::thread::{self, JoinHandle};
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;
#[cfg(not(windows))]
use std::process::Command;


#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn CreateJobObjectW(attributes: *mut std::ffi::c_void, name: *const u16) -> *mut std::ffi::c_void;
    fn SetInformationJobObject(
        job: *mut std::ffi::c_void,
        class: i32,
        info: *const std::ffi::c_void,
        info_len: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: *mut std::ffi::c_void, process: *mut std::ffi::c_void) -> i32;
    fn TerminateJobObject(job: *mut std::ffi::c_void, exit_code: u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> *mut std::ffi::c_void;
    fn Thread32First(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn Thread32Next(snapshot: *mut std::ffi::c_void, entry: *mut ThreadEntry32) -> i32;
    fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> *mut std::ffi::c_void;
    fn ResumeThread(thread: *mut std::ffi::c_void) -> u32;
}

pub const MAX_CAPTURE_BYTES_PER_STREAM: usize = 1024 * 1024;
#[cfg(windows)]
const READER_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const READER_KILL_GRACE: Duration = Duration::from_millis(750);

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub timed_out: bool,
    pub drain_timed_out: bool,
}

impl CapturedOutput {
    pub fn combined_text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&self.stderr);
        if !stderr.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(stderr.trim_end());
        }
        if self.stdout_truncated || self.stderr_truncated {
            text.push_str("\n[NOVA] Output was truncated at 1 MiB per stream.");
        }
        if self.drain_timed_out {
            text.push_str("\n[NOVA] Output collection stopped after process-tree termination and drain timeout.");
        }
        if self.timed_out {
            text.push_str("\n[NOVA] Command timed out; the Windows process job was terminated.");
        }
        if text.is_empty() && !self.status.success() {
            text = format!("Command exited with code {}.", self.status.code().unwrap_or(-1));
        }
        text
    }
}


/// Resolve an executable from the actual Windows system directory, not PATH
/// or the process working directory. This avoids accidentally launching a
/// same-named executable from user-controlled locations.
#[cfg(windows)]
pub fn system_binary(name: &str) -> io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = vec![0u16; 32_768];
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= buffer.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows system directory path exceeded the supported buffer length.",
        ));
    }
    let directory: std::path::PathBuf = std::ffi::OsString::from_wide(&buffer[..length]).into();
    Ok(directory.join(name))
}

/// Run a command with stdout/stderr continuously drained, capped capture, a
/// wall-clock timeout and a Windows Job Object for process-tree cleanup.
#[cfg(windows)]
pub fn run_captured(mut command: Command, timeout: Duration) -> io::Result<CapturedOutput> {
    let job = ProcessJob::new()?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Start suspended so no child process can be created before it belongs to
    // the Job Object. The initial thread is resumed only after successful
    // assignment; if either operation fails, the process is killed fail-closed.
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let mut child = command.spawn()?;
    if let Err(error) = job.assign(&child).and_then(|_| resume_suspended_process(child.id())) {
        job.terminate();
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let stdout = child.stdout.take().ok_or_else(|| io::Error::other("stdout pipe was not created"))?;
    let stderr = child.stderr.take().ok_or_else(|| io::Error::other("stderr pipe was not created"))?;
    let out_reader = thread::spawn(move || drain_limited(stdout));
    let err_reader = thread::spawn(move || drain_limited(stderr));

    let deadline = Instant::now() + timeout;
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status, false),
            Ok(None) => {}
            Err(error) => {
                job.terminate();
                let _ = child.kill();
                let _ = child.wait();
                let _ = wait_for_readers(&out_reader, &err_reader, READER_KILL_GRACE);
                let _ = join_reader(out_reader, "stdout");
                let _ = join_reader(err_reader, "stderr");
                return Err(error);
            }
        }

        if Instant::now() >= deadline {
            job.terminate();
            let _ = child.kill();
            let status = child.wait()?;
            break (status, true);
        }
        thread::sleep(Duration::from_millis(40));
    };

    let mut readers_finished = wait_for_readers(&out_reader, &err_reader, READER_DRAIN_TIMEOUT);
    if !readers_finished {
        // Some grandchildren can inherit pipe handles after the command shell
        // exits. Terminating the job closes those handles for normal children.
        job.terminate();
        let _ = child.kill();
        readers_finished = wait_for_readers(&out_reader, &err_reader, READER_KILL_GRACE);
    }

    let (stdout, stdout_truncated) = join_reader(out_reader, "stdout")?;
    let (stderr, stderr_truncated) = join_reader(err_reader, "stderr")?;

    // Closing `job` below is a final safety net: KILL_ON_JOB_CLOSE terminates
    // every process still assigned to this job, including lingering children.
    drop(job);

    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        timed_out,
        drain_timed_out: !readers_finished,
    })
}

#[cfg(windows)]
fn join_reader(
    reader: JoinHandle<io::Result<(Vec<u8>, bool)>>,
    stream_name: &str,
) -> io::Result<(Vec<u8>, bool)> {
    if !reader.is_finished() {
        // The job has already been terminated. If a process escaped its job,
        // detach the reader rather than making the UI or worker wait forever.
        drop(reader);
        return Ok((Vec::new(), true));
    }
    reader
        .join()
        .map_err(|_| io::Error::other(format!("NOVA {stream_name} reader thread panicked")))?
}

#[cfg(windows)]
fn wait_for_readers(
    stdout: &JoinHandle<io::Result<(Vec<u8>, bool)>>,
    stderr: &JoinHandle<io::Result<(Vec<u8>, bool)>>,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while (!stdout.is_finished() || !stderr.is_finished()) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    stdout.is_finished() && stderr.is_finished()
}

/// Read to EOF even after the capture cap, otherwise a full OS pipe can block
/// the child that is writing to it. This function is platform independent so
/// its critical behavior can be unit-tested on non-Windows hosts too.
pub fn drain_limited<R: Read>(mut reader: R) -> io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(MAX_CAPTURE_BYTES_PER_STREAM);
    let mut buffer = [0u8; 8192];
    let mut truncated = false;

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_CAPTURE_BYTES_PER_STREAM.saturating_sub(retained.len());
        let keep = count.min(remaining);
        retained.extend_from_slice(&buffer[..keep]);
        truncated |= keep < count;
    }
    Ok((retained, truncated))
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct ThreadEntry32 {
    size: u32,
    usage: u32,
    thread_id: u32,
    owner_process_id: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

#[cfg(windows)]
fn resume_suspended_process(process_id: u32) -> io::Result<()> {
    const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
    const THREAD_SUSPEND_RESUME: u32 = 0x0002;
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot as isize == -1isize {
        return Err(io::Error::last_os_error());
    }

    let mut entry = ThreadEntry32 {
        size: std::mem::size_of::<ThreadEntry32>() as u32,
        ..Default::default()
    };
    let mut found_thread = std::ptr::null_mut();
    let mut enumeration_ok = unsafe { Thread32First(snapshot, &mut entry) };
    while enumeration_ok != 0 {
        if entry.owner_process_id == process_id {
            found_thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.thread_id) };
            break;
        }
        enumeration_ok = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    let enumeration_error = if found_thread.is_null() {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    unsafe { let _ = CloseHandle(snapshot); }

    if let Some(error) = enumeration_error {
        return Err(error);
    }
    let previous_suspend_count = unsafe { ResumeThread(found_thread) };
    let resume_error = if previous_suspend_count == u32::MAX {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    unsafe { let _ = CloseHandle(found_thread); }
    if let Some(error) = resume_error {
        return Err(error);
    }
    Ok(())
}

#[cfg(windows)]
struct ProcessJob {
    handle: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl ProcessJob {
    fn new() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut info = JobObjectExtendedLimitInformation::default();
        info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&info as *const JobObjectExtendedLimitInformation).cast(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            unsafe { let _ = CloseHandle(handle); }
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn assign(&self, child: &Child) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        let ok = unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle().cast()) };
        if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    fn terminate(&self) {
        unsafe { let _ = TerminateJobObject(self.handle, 1); }
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { let _ = CloseHandle(self.handle); }
            self.handle = std::ptr::null_mut();
        }
    }
}

#[cfg(windows)]
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
#[cfg(windows)]
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

#[cfg(test)]
mod tests {
    use super::{drain_limited, MAX_CAPTURE_BYTES_PER_STREAM};
    use std::io::{self, Cursor, Read};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    struct CountingReader {
        inner: Cursor<Vec<u8>>,
        total_read: Arc<AtomicUsize>,
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.inner.read(buffer)?;
            self.total_read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    #[test]
    fn drain_limited_reads_to_eof_after_capture_cap() {
        let data = vec![b'x'; MAX_CAPTURE_BYTES_PER_STREAM + 128 * 1024];
        let expected = data.len();
        let total_read = Arc::new(AtomicUsize::new(0));
        let reader = CountingReader { inner: Cursor::new(data), total_read: Arc::clone(&total_read) };
        let (captured, truncated) = drain_limited(reader).unwrap();
        assert_eq!(captured.len(), MAX_CAPTURE_BYTES_PER_STREAM);
        assert!(truncated);
        assert_eq!(total_read.load(Ordering::Relaxed), expected);
    }

    #[test]
    fn drain_limited_keeps_small_stream_intact() {
        let input = b"hello NOVA";
        let (captured, truncated) = drain_limited(Cursor::new(input)).unwrap();
        assert_eq!(captured, input);
        assert!(!truncated);
    }

    #[cfg(windows)]
    #[test]
    fn job_object_runner_captures_a_windows_child() {
        use std::process::Command;
        use std::time::Duration;
        let mut command = Command::new(super::system_binary("cmd.exe").unwrap());
        command.args(["/D", "/S", "/C", "echo NOVA_JOB_OBJECT_TEST"]);
        let output = super::run_captured(command, Duration::from_secs(5)).unwrap();
        assert!(output.status.success());
        assert!(!output.timed_out);
        assert!(String::from_utf8_lossy(&output.stdout).contains("NOVA_JOB_OBJECT_TEST"));
    }

    #[cfg(windows)]
    #[test]
    fn job_object_runner_drains_output_after_capture_limit() {
        use std::process::Command;
        use std::time::Duration;
        let mut command = Command::new(super::system_binary("cmd.exe").unwrap());
        command.args([
            "/D", "/S", "/C",
            "for /L %i in (1,1,40000) do @echo 0123456789abcdef0123456789abcdef",
        ]);
        let output = super::run_captured(command, Duration::from_secs(15)).unwrap();
        assert!(output.status.success());
        assert!(!output.timed_out);
        assert!(output.stdout_truncated);
        assert_eq!(output.stdout.len(), super::MAX_CAPTURE_BYTES_PER_STREAM);
    }

    #[cfg(windows)]
    #[test]
    fn job_object_runner_terminates_a_timed_out_child() {
        use std::process::Command;
        use std::time::Duration;
        let mut command = Command::new(super::system_binary("ping.exe").unwrap());
        command.args(["-n", "20", "127.0.0.1"]);
        let output = super::run_captured(command, Duration::from_millis(250)).unwrap();
        assert!(output.timed_out);
    }
}

#[cfg(not(windows))]
pub fn run_captured(_command: Command, _timeout: Duration) -> io::Result<CapturedOutput> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Windows process execution is available only in the Windows build."))
}

