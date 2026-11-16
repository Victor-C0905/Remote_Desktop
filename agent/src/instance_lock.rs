//! Manager 单实例锁（R6-?-01）
//!
//! 背景：QUIC socket 的 SO_REUSEADDR 是 restart 端口释放延迟的既有方案，
//! 但在 Linux 下它让第二个进程**静默**绑定同一 UDP 端口成功——陈旧实例
//! 继续持有客户端已建立的 QUIC 连接并服务请求（探测结果陈旧，R6-?-01 的
//! 「目录已建仍返回 0、kill 全部进程后变 2」），新实例空转，日志无任何异常。
//!
//! 本锁把该场景转为大声失败：flock 独占锁，锁文件置于配置文件同目录
//! （transfer_limit 的配置写回已保证配置目录可写），按 QUIC 端口命名——
//! 端口是冲突资源的最小标识，不同端口的多实例（测试探针场景）互不影响。
//! flock 随持有进程死亡（含 SIGKILL）由内核自动释放，无陈锁问题。
//!
//! 注：systemd unit 的 `KillMode=process` 是 Phase 4 热更新的刻意设计
//! （systemctl stop 由 Manager 优雅关闭 Worker，避免误杀中断 PTY 会话），
//! 本锁不改变它；补的是「Manager 硬死/半失败重启」时第二实例的静默共存。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

/// 实例锁获取失败
#[derive(Debug, thiserror::Error)]
pub enum InstanceLockError {
    /// 另一实例持有锁（pid 为尽力读取，供诊断）
    #[error("另一 quireld 实例正在运行（PID {pid:?}），QUIC 端口 {port} 已被占用（锁文件 {path}）；如确认无运行实例，请结束旧进程后重试")]
    Held {
        pid: Option<u32>,
        port: u16,
        path: PathBuf,
    },

    #[error("实例锁文件操作失败: {0}")]
    Io(#[from] std::io::Error),
}

/// 实例锁守卫：持有期间锁有效；drop（进程退出/被杀）时内核自动释放 flock
#[derive(Debug)]
pub struct InstanceGuard {
    _file: File,
}

/// 获取 Manager 单实例锁。
///
/// `config_dir`：配置文件所在目录（锁文件写入此处）；
/// `port`：QUIC 监听端口（锁文件命名的冲突资源标识）。
pub fn acquire(config_dir: &Path, port: u16) -> Result<InstanceGuard, InstanceLockError> {
    fs::create_dir_all(config_dir)?;
    let path = config_dir.join(format!("quireld-manager-{port}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)?;

    // 非阻塞独占 flock：持有者存活即拒绝；持有者死亡（含 SIGKILL）
    // 由内核自动释放，无陈锁
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        if err.kind() == std::io::ErrorKind::WouldBlock {
            let pid = read_pid(&path);
            return Err(InstanceLockError::Held { pid, port, path });
        }
        return Err(err.into());
    }

    // 记录本进程 pid（尽力而为，供冲突方诊断）
    let mut f = &file;
    f.set_len(0)?;
    write!(f, "{}", std::process::id())?;
    f.flush()?;

    Ok(InstanceGuard { _file: file })
}

/// 尽力读取锁文件中记录的持有者 pid（供冲突诊断；缺失/损坏返回 None）
fn read_pid(path: &Path) -> Option<u32> {
    let mut buf = String::new();
    File::open(path).ok()?.read_to_string(&mut buf).ok()?;
    buf.trim().parse().ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// 独立临时目录：按进程 id + 标签隔离，互不干扰
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quireld-instance-lock-test-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("创建测试临时目录");
        dir
    }

    #[test]
    fn 同端口第二实例获取失败并报对方pid() {
        let dir = temp_dir("held");
        let _guard = acquire(&dir, 8443).expect("首次获取应成功");
        let err = acquire(&dir, 8443).expect_err("第二实例应被拒绝");
        match err {
            InstanceLockError::Held { pid, .. } => {
                assert_eq!(pid, Some(std::process::id()), "锁文件应记录持有者 pid")
            }
            other => panic!("期望 Held，实际 {other:?}"),
        }
    }

    #[test]
    fn 守卫释放后可重新获取() {
        let dir = temp_dir("release");
        {
            let _guard = acquire(&dir, 8444).expect("首次获取应成功");
        } // drop → flock 释放
        acquire(&dir, 8444).expect("释放后再次获取应成功");
    }

    #[test]
    fn 不同端口互不影响() {
        let dir = temp_dir("ports");
        let _g1 = acquire(&dir, 9001).expect("端口 9001 应成功");
        let _g2 = acquire(&dir, 9002).expect("端口 9002 应成功");
    }
}
