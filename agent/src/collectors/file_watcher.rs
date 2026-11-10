//! 目录变更采集器（inotify → EventBus 文件级增量）
//!
//! 背景：文件管理器只靠自身操作后的增量补丁，无法覆盖解压/编辑器保存/
//! 终端/其他客户端对同一目录的修改。本采集器按订阅监听当前可见目录，
//! 把变更**按条目归并**后推送 `file_changes` 事件（不是“目录变了你自己重读”），
//! 弱网/大目录下只传变更条目，避免整目录 read_dir 的流量与延迟。
//!
//! 关键约束：
//! - 非递归（文件管理器只展示直属条目）；
//! - 变更条目必须**以目标用户身份** stat（root 视角会泄露无权限条目的元数据）；
//! - 100ms 合并窗口 + 单批上限，超过则回退 resync（由客户端整目录重读）；
//! - 无订阅时不停机：watcher 常驻，仅增减 watch（避免反复创建线程）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::{json, Value};

use crate::auth::{UserExecutor, UserSession};
use crate::event_bus::EventBus;
use crate::file_stream::is_transfer_temp_name;
use crate::worker::handlers::file::{
    build_gid_name_map, build_uid_name_map, entry_from_metadata, FileEntryData,
};

/// 单次增量推送的条目上限：超过（典型是解压几千个文件）直接 resync，
/// 一次整目录读取比推几千条增量更省
const MAX_DELTA_ENTRIES: usize = 200;

/// resync 最小间隔：批量操作（解压/大量小文件上传完成）会连续触发超限，
/// 不节流会变成每秒多次整目录读取（大目录 + 弱网直接卡死）
const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// 合并窗口：同一窗口内的多次事件按条目名归并后只推一次
const DEFAULT_DELAY_MS: u64 = 100;

/// 监听循环的最小调度间隔（合并窗口的检查粒度）
const TICK_MS: u64 = 50;

/// 一个被监听目录的注册信息
#[derive(Clone)]
struct WatchSpec {
    /// 客户端订阅时使用的逻辑路径（事件里回传它，不能回传规范化后的真实路径，
    /// 否则前端 currentPath 对不上）
    display: PathBuf,
    /// stat 必须在该用户上下文中执行
    session: UserSession,
}

/// 单目录的待推送批次
#[derive(Default)]
struct Batch {
    upserts: HashSet<String>,
    removes: HashSet<String>,
    renames: Vec<(String, String)>,
    resync: bool,
    first_seen: Option<Instant>,
}

impl Batch {
    fn touch(&mut self) {
        if self.first_seen.is_none() {
            self.first_seen = Some(Instant::now());
        }
    }

    fn len(&self) -> usize {
        self.upserts.len() + self.removes.len() + self.renames.len()
    }

    fn is_empty(&self) -> bool {
        !self.resync && self.len() == 0
    }
}

/// 目录变更采集器（跨订阅共享一个 inotify watcher）
pub struct FileWatcher {
    inner: Arc<Inner>,
}

struct Inner {
    event_bus: Arc<EventBus>,
    delay: Duration,
    /// watch 路径（validate_path 规范化后）→ 展示路径 + 用户身份
    desired: Mutex<HashMap<PathBuf, WatchSpec>>,
    started: tokio::sync::Mutex<bool>,
}

impl FileWatcher {
    pub fn new(delay_ms: u64, event_bus: Arc<EventBus>) -> Self {
        let delay = Duration::from_millis(if delay_ms == 0 { DEFAULT_DELAY_MS } else { delay_ms });
        Self {
            inner: Arc::new(Inner {
                event_bus,
                delay,
                desired: Mutex::new(HashMap::new()),
                started: tokio::sync::Mutex::new(false),
            }),
        }
    }

    /// 注册一个目录监听（首次调用时启动常驻 watcher 任务）。
    /// `watch_path` 是校验/规范化后的真实路径；`display` 是客户端逻辑路径。
    pub async fn watch(&self, watch_path: PathBuf, display: PathBuf, session: UserSession) {
        {
            let mut desired = self.inner.desired.lock().unwrap();
            if desired.contains_key(&watch_path) {
                return;
            }
            desired.insert(watch_path, WatchSpec { display, session });
        }
        self.ensure_started().await;
    }

    /// 取消监听（引用计数归零时由 SubscriptionManager 调用）
    pub fn unwatch(&self, watch_path: &Path) {
        let mut desired = self.inner.desired.lock().unwrap();
        desired.remove(watch_path);
    }

    async fn ensure_started(&self) {
        let mut started = self.inner.started.lock().await;
        if *started {
            return;
        }
        *started = true;
        let inner = self.inner.clone();
        tokio::spawn(async move { run(inner).await });
    }
}

/// 常驻循环：同步 watch 集合 + 收集事件 + 合并窗口到期后推送
async fn run(inner: Arc<Inner>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Result<Event>>();
    let mut watcher = match RecommendedWatcher::new(
        move |res: notify::Result<Event>| {
            // 回调线程只负责把事件送回 tokio 任务；通道关闭说明任务已退出
            let _ = tx.send(res);
        },
        Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("创建文件监听器失败: {}", e);
            return;
        }
    };

    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut batches: HashMap<PathBuf, Batch> = HashMap::new();
    // 每目录最近一次 resync 时间（节流用）
    let mut last_resync: HashMap<PathBuf, Instant> = HashMap::new();
    let mut tick = tokio::time::interval(Duration::from_millis(TICK_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                sync_watches(&inner, &mut watcher, &mut watched, &mut batches);
                flush_ready(&inner, &mut batches, &mut last_resync).await;
            }
            maybe = rx.recv() => {
                let Some(res) = maybe else {
                    tracing::warn!("文件监听事件通道已关闭，采集器退出");
                    return;
                };
                match res {
                    Ok(event) => on_event(&inner, event, &mut batches),
                    Err(e) => {
                        // 事件丢失（含 inotify 队列溢出/上限）：对全部目录置 resync，
                        // 客户端收到后整目录重读，避免静默漏更新
                        tracing::warn!("文件监听错误，全部目录标记重同步: {}", e);
                        for path in &watched {
                            batches.entry(path.clone()).or_default().resync = true;
                            batches.entry(path.clone()).or_default().touch();
                        }
                    }
                }
            }
        }
    }
}

/// 把 desired 的增减同步到 inotify（新增 watch / 移除旧 watch）
fn sync_watches(
    inner: &Inner,
    watcher: &mut RecommendedWatcher,
    watched: &mut HashSet<PathBuf>,
    batches: &mut HashMap<PathBuf, Batch>,
) {
    let desired: HashSet<PathBuf> = {
        let map = inner.desired.lock().unwrap();
        map.keys().cloned().collect()
    };

    for path in desired.difference(watched) {
        match watcher.watch(path, RecursiveMode::NonRecursive) {
            Ok(()) => {
                tracing::debug!("目录监听已注册: {}", path.display());
            }
            Err(e) => {
                tracing::warn!("目录监听注册失败: {}: {}", path.display(), e);
            }
        }
    }
    for path in watched.difference(&desired).cloned().collect::<Vec<_>>() {
        if let Err(e) = watcher.unwatch(&path) {
            tracing::debug!("目录监听取消失败(可能已移除): {}: {}", path.display(), e);
        }
        batches.remove(&path);
    }
    *watched = desired;
}

/// 把一个 notify 事件归并进对应目录的批次
fn on_event(inner: &Inner, event: Event, batches: &mut HashMap<PathBuf, Batch>) {
    let desired = inner.desired.lock().unwrap();

    for path in &event.paths {
        // 目录自身的事件（属性变化/被移动）：无法定位到具体条目 → resync
        if desired.contains_key(path) {
            let batch = batches.entry(path.clone()).or_default();
            batch.resync = true;
            batch.touch();
            continue;
        }

        let Some(dir) = path.parent() else { continue };
        if !desired.contains_key(dir) {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        // 上传临时文件（quireld_*.tmp）不进入用户视图
        if is_transfer_temp_name(&name) {
            continue;
        }

        let batch = batches.entry(dir.to_path_buf()).or_default();
        batch.touch();
        match &event.kind {
            EventKind::Create(_) => {
                batch.upserts.insert(name);
            }
            EventKind::Remove(_) => {
                batch.removes.insert(name);
            }
            EventKind::Modify(ModifyKind::Name(mode)) => match mode {
                RenameMode::From => {
                    batch.removes.insert(name);
                }
                RenameMode::To => {
                    batch.upserts.insert(name);
                }
                // Both 由事件末尾的双路径统一处理；Any/Other 无法判定 → resync
                RenameMode::Both => {}
                RenameMode::Any | RenameMode::Other => {
                    batch.resync = true;
                }
            },
            EventKind::Modify(_) => {
                batch.upserts.insert(name);
            }
            // 写入关闭（编辑器保存）只保证 mtime/大小变化，同样按条目刷新
            EventKind::Access(AccessKind::Close(AccessMode::Write)) => {
                batch.upserts.insert(name);
            }
            EventKind::Access(_) => {}
            EventKind::Any | EventKind::Other => {
                batch.resync = true;
            }
        }
    }

    // 改名：notify 的 Both 事件带 [from, to] 两个路径（同目录内）
    if let EventKind::Modify(ModifyKind::Name(RenameMode::Both)) = event.kind {
        if event.paths.len() >= 2 {
            let from = &event.paths[0];
            let to = &event.paths[1];
            if from.parent() == to.parent() {
                if let (Some(dir), Some(from_name), Some(to_name)) = (
                    from.parent(),
                    from.file_name().map(|n| n.to_string_lossy().to_string()),
                    to.file_name().map(|n| n.to_string_lossy().to_string()),
                ) {
                    if desired.contains_key(dir) {
                        let batch = batches.entry(dir.to_path_buf()).or_default();
                        batch.touch();
                        batch.removes.remove(&from_name);
                        batch.removes.remove(&to_name);
                        batch.upserts.remove(&to_name);
                        // 上传完成：临时文件 rename 成正式名。临时文件本就不展示，
                        // 等价于“新文件出现”，不能生成 from=临时名的 rename
                        if is_transfer_temp_name(&from_name) {
                            batch.upserts.insert(to_name);
                        } else if !is_transfer_temp_name(&to_name) {
                            batch.renames.push((from_name, to_name));
                        }
                    }
                }
            }
        }
    }
}

/// 到期批次：构造文件级增量并发布 `file_changes`
async fn flush_ready(
    inner: &Inner,
    batches: &mut HashMap<PathBuf, Batch>,
    last_resync: &mut HashMap<PathBuf, Instant>,
) {
    let now = Instant::now();
    let ready: Vec<PathBuf> = batches
        .iter()
        .filter(|(dir, b)| {
            let due = b
                .first_seen
                .map(|t| now.duration_since(t) >= inner.delay)
                .unwrap_or(false)
                || b.len() > MAX_DELTA_ENTRIES;
            if !due {
                return false;
            }
            // resync 类批次受每目录最小间隔约束：未到间隔继续积累事件，
            // 到期后一次全量重读（节流，而不是每秒多次 read_dir）
            let heavy = b.resync || b.len() > MAX_DELTA_ENTRIES;
            if heavy {
                // filter 的闭包参数是 &Item：dir 实际为 &&PathBuf，解引用后再取键
                if let Some(last) = last_resync.get(*dir) {
                    if now.duration_since(*last) < RESYNC_MIN_INTERVAL {
                        return false;
                    }
                }
            }
            true
        })
        .map(|(p, _)| p.clone())
        .collect();

    for dir in ready {
        let Some(batch) = batches.remove(&dir) else { continue };
        if batch.is_empty() {
            continue;
        }
        let spec = {
            let map = inner.desired.lock().unwrap();
            map.get(&dir).cloned()
        };
        let Some(spec) = spec else { continue };
        flush_batch(inner, dir, batch, spec, last_resync).await;
    }
}

async fn flush_batch(
    inner: &Inner,
    dir: PathBuf,
    batch: Batch,
    spec: WatchSpec,
    last_resync: &mut HashMap<PathBuf, Instant>,
) {
    let display = spec.display.to_string_lossy().to_string();

    // 超限或目录自身变化：只发 resync，客户端整目录重读
    if batch.resync || batch.len() > MAX_DELTA_ENTRIES {
        last_resync.insert(dir, Instant::now());
        inner
            .event_bus
            .publish("file_changes", json!({ "dir": display, "resync": true }))
            .await;
        return;
    }

    // 需要元数据的名字：新增/改名目标；改名源只做删除
    let mut stat_names: Vec<String> = batch.upserts.iter().cloned().collect();
    for (_, to) in &batch.renames {
        stat_names.push(to.clone());
    }
    stat_names.sort();
    stat_names.dedup();

    let entries = match stat_as_user(&dir, &stat_names, &spec.session).await {
        Ok(entries) => entries,
        Err(e) => {
            // stat 失败（fork/权限/目录已消失）：不能把未知状态当删除，
            // 直接让客户端整目录重读（同样受限速约束）
            tracing::warn!("文件变更 stat 失败，转为 resync: {}: {}", dir.display(), e);
            last_resync.insert(dir, Instant::now());
            inner
                .event_bus
                .publish("file_changes", json!({ "dir": display, "resync": true }))
                .await;
            return;
        }
    };

    let mut found: HashMap<String, FileEntryData> = HashMap::new();
    for (name, entry) in entries {
        found.insert(name, entry);
    }

    let mut upserts: Vec<Value> = Vec::new();
    let mut removes: Vec<String> = batch.removes.into_iter().collect();

    for name in batch.upserts {
        match found.remove(&name) {
            Some(entry) => match serde_json::to_value(&entry) {
                Ok(v) => upserts.push(v),
                Err(e) => tracing::warn!("文件条目序列化失败: {}: {}", name, e),
            },
            // 窗口内又被删除：按删除处理
            None => removes.push(name),
        }
    }

    let mut renames: Vec<Value> = Vec::new();
    for (from, to) in batch.renames {
        match found.remove(&to) {
            Some(entry) => match serde_json::to_value(&entry) {
                Ok(v) => renames.push(json!({ "from": from, "to": to, "entry": v })),
                Err(e) => tracing::warn!("改名条目序列化失败: {}: {}", to, e),
            },
            None => removes.push(to),
        }
    }

    removes.sort();
    removes.dedup();

    if upserts.is_empty() && renames.is_empty() && removes.is_empty() {
        return;
    }

    inner
        .event_bus
        .publish(
            "file_changes",
            json!({
                "dir": display,
                "upserts": upserts,
                "removes": removes,
                "renames": renames,
            }),
        )
        .await;
}

/// 以目标用户身份批量 stat（一次 fork 处理整批）
async fn stat_as_user(
    dir: &Path,
    names: &[String],
    session: &UserSession,
) -> anyhow::Result<Vec<(String, FileEntryData)>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let executor = UserExecutor::new(session);
    let dir_str = dir.to_string_lossy().to_string();
    let names = names.to_vec();

    executor
        .execute_as_user_blocking(move || {
            let uid_names = build_uid_name_map();
            let gid_names = build_gid_name_map();
            let base = Path::new(&dir_str);
            let mut out: Vec<(String, FileEntryData)> = Vec::new();
            for name in names {
                // 与 read_dir 口径一致：DirEntry::metadata 不跟随符号链接
                if let Ok(md) = std::fs::symlink_metadata(base.join(&name)) {
                    out.push((
                        name.clone(),
                        entry_from_metadata(name, &md, &uid_names, &gid_names),
                    ));
                }
            }
            Ok(out)
        })
        .await
}
