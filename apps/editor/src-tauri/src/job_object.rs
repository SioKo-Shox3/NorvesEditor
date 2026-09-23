//! Windows の Job Object で、エディタが起動したエンジンをエディタの寿命に縛る。
//!
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` を付けた Job に子プロセスを割り当てておくと、Job の
//! 最後のハンドルが閉じた時点で OS が Job 内のプロセスをすべて終了させる。エディタが強制終了
//! (タスクマネージャからの終了・クラッシュ)しても、OS がエディタのハンドルを閉じるので
//! エンジンは残らない。ハンドルを持つのは [`KillOnCloseJob`] だけで、`Drop` で閉じる。

use std::ffi::c_void;
use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// ハンドルが閉じると中のプロセスを全部終了させる、名前の無い Job。
pub struct KillOnCloseJob {
    handle: HANDLE,
}

// SAFETY: カーネルオブジェクトのハンドルはスレッドをまたいで使ってよい。`handle` を閉じるのは
// `Drop` だけで、それ以外は読み取るだけなので、共有しても二重に閉じることはない。
unsafe impl Send for KillOnCloseJob {}
// SAFETY: 同上。`assign` は `&self` からハンドルを渡すだけで、Job 側の操作は OS が直列化する。
unsafe impl Sync for KillOnCloseJob {}

impl KillOnCloseJob {
    /// 名前の無い Job を作り、`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` を設定する。
    pub fn new() -> io::Result<Self> {
        // SAFETY: 既定のセキュリティ属性・名前なしで作る。失敗時は null が返る。
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // ここから先で失敗しても `Drop` がハンドルを閉じる。
        let job = KillOnCloseJob { handle };

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `info` は呼び出しの間生きていて、渡す長さはその型の大きさと一致する。
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// プロセスを Job に割り当てる。`process` は呼び出しの間有効なプロセスハンドルであること
    /// (`PROCESS_SET_QUOTA` と `PROCESS_TERMINATE` の権限が要る。自分で起動した子なら持っている)。
    pub fn assign(&self, process: std::os::windows::io::RawHandle) -> io::Result<()> {
        // SAFETY: `self.handle` は `Drop` まで有効。`process` の有効性は呼び出し側が保証する。
        let ok = unsafe { AssignProcessToJobObject(self.handle, process as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        // SAFETY: `handle` は `new` で得た有効なハンドルで、閉じるのはここだけ。
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    /// 数十秒は自分から終わらない子プロセスを起動する。
    fn spawn_long_lived() -> Child {
        Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("ping を起動できない")
    }

    /// `deadline` までに子が終われば true。
    fn exits_within(child: &mut Child, deadline: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if child.try_wait().expect("try_wait に失敗").is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn closing_the_job_terminates_the_assigned_child() {
        let mut child = spawn_long_lived();
        let job = KillOnCloseJob::new().expect("Job を作れない");
        job.assign(child.as_raw_handle())
            .expect("Job に割り当てられない");

        // 閉じる前は生きている(割り当てただけでは終わらない)。
        assert!(child.try_wait().expect("try_wait に失敗").is_none());

        drop(job);
        let exited = exits_within(&mut child, Duration::from_secs(5));
        if !exited {
            let _ = child.kill();
        }
        assert!(exited, "Job を閉じても子が 5 秒以内に終了しなかった");
    }

    #[test]
    fn assigning_an_invalid_handle_is_an_error() {
        // 割り当ての失敗は Err で返る(呼び出し側はこれを警告にして起動を続ける)。
        let job = KillOnCloseJob::new().expect("Job を作れない");
        assert!(job.assign(std::ptr::null_mut()).is_err());
    }

    #[test]
    fn unassigned_child_survives_closing_the_job() {
        // 対照: 割り当てていない子は Job を閉じても生き残る(上のテストが偶然通っていないことの確認)。
        let mut child = spawn_long_lived();
        let job = KillOnCloseJob::new().expect("Job を作れない");
        drop(job);
        let exited = exits_within(&mut child, Duration::from_millis(500));
        let _ = child.kill();
        let _ = child.wait();
        assert!(!exited, "割り当てていない子が終了した");
    }
}
