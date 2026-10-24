//! 地址栏搜索联想（suggest）
//!
//! 数据通路：本机 SOCKS5 监听端口（proxy 会话）→ QUIC 隧道 → Agent →
//! 搜索引擎 suggest 端点。与页面浏览同隧道同出口 IP：
//! - 本地网络无法直达搜索引擎（如 Google 被墙）时联想照常可用
//! - CONNECT 透传域名 → DNS 在 Agent 侧解析，本地无 DNS 查询泄漏
//!
//! 连接复用：与 suggest 端点保持多条 TLS 长连接（HTTP/1.1 keep-alive，
//! 按 端口+主机 分池，每池深度 POOL_DEPTH）。HTTP/1.1 一条连接同刻只能
//! 服务一个请求——快速输入时上一击键的查询还在途并占着连接，池深 1 时
//! 本击键必然落冷建连（Agent 拨号 + TLS 握手 ≈ 3×隧道 RTT，实测为
//! "联想比外部浏览器慢"的根因：Chrome 经同一隧道用 HTTP/2 多路复用 +
//! 6 路预连接，任意击键恒 1 RTT）。池深 N + "池空短等归还"对齐该体验：
//! 在途请求读完响应即归还（≈1 RTT），池空等待通常 <1 RTT 拿到连接，
//! 冷建连只剩兜底场景。
//! 取用即从池移出，仅完整收到响应且连接可续用时归还；任何失败/超时
//! 连接随之丢弃，下次自动重建。
//!
//! 熔断：搜索引擎在某个 Agent 出口下可能整体不可达（实测 Google 曾出现
//! TCP SYN 黑洞——请求挂满超时窗口，而同出口 Bing 0.2s 正常）。此时每次
//! 击键都白等一个超时窗口，表现为"联想极慢"。连续失败达阈值后按
//! (服务器, 引擎) 熔断冷却 30s，冷却期内直接返回空；冷却结束自动试探
//! 恢复，成功即复位。按服务器分键是因为各出口可达性互不相同，且用户
//! 会在多服务器间切换，跨服务器共享会误熔断。
//!
//! 失败语义：任何环节失败一律返回空列表（联想是装饰性功能，静默降级并
//! 记日志），前端不区分"无联想词"与"隧道不可用"。
//!
//! 快进快出三件套（Chrome Omnibox 同款思路）：① 2.5s 超时——高于跨洋
//! 冷请求实测值（~1.6s），热请求远低于此；② 无请求级取消——过期查询
//! 自然跑完，结果由前端 seq 丢弃，连接完整归还（abort 取消会连带杀掉
//! 该请求持有的池连接，实测为"预热从不命中"的根因）；③ 结果缓存
//! （TTL 5min、容量 64）——同前缀重复输入秒出，不再走隧道。熔断仅兜底
//! "持续不可达"。
//!
//! 协议实现保持最小：SOCKS5 客户端握手（与 socks5.rs 服务端同协议对端）
//! + 手写 HTTP/1.1 响应定界（Content-Length / chunked / 关流三种）+
//! JSON 取 `[query, [suggestions…]]` 二元结构的第二项。两个引擎
//! （Google client=firefox / Bing osjson）响应结构一致。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::{Lazy, OnceCell};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_rustls::TlsConnector;

use super::browser_settings::{percent_encode, SearchEngine};
use super::ProxySessionManager;

/// 整体超时：连接 + SOCKS5 握手 + TLS 握手 + HTTP 响应。
/// 联想是装饰性功能，宁可没词也不能拖输入，但下限由物理路径决定：
/// 跨洋隧道冷请求实测 ~1.6s（本地→QUIC→Agent 拨号→TLS→HTTP ≈ 5 个
/// 往返），2.5s = 实测值 + 余量；热请求（长连接复用）仅 1 RTT 不受影响。
/// 再短会误杀正常冷请求并连锁误触发熔断（实测教训：1.5s 恰好低于
/// 泰国出口 Google 冷请求的 1.55s）。
const FETCH_TIMEOUT: Duration = Duration::from_millis(2500);
/// 单次 read 超时（正常单次读 ≤1 RTT；用于保护池中连接不被外层超时连坐）
const READ_TIMEOUT: Duration = Duration::from_secs(3);

/// 连续失败达此值触发熔断
const BREAK_THRESHOLD: u32 = 2;
/// 熔断冷却时长
const BREAK_COOLDOWN: Duration = Duration::from_secs(30);

/// 单次响应体上限（联想 JSON 仅几 KB，此为防御性上限）
const MAX_BODY: usize = 1024 * 1024;
/// 响应头上限
const MAX_HEAD: usize = 64 * 1024;
/// 每池连接上限（键为 端口+主机）。深度 3 覆盖常规打字并发：
/// 防抖 40ms + 跨洋 RTT ~400ms → 同时在途约 2-3 个查询各需一条连接
const POOL_DEPTH: usize = 3;
/// 池空时等待在途请求归还的时长（≈1 个跨洋 RTT）：旧请求读完响应即
/// 归还连接，等待远短于冷建连（Agent 拨号 + TLS 握手 ≈ 3+ RTT）；
/// 等不到再冷建连兜底
const POOL_WAIT: Duration = Duration::from_millis(450);

/// TLS 连接器（含 OS 根证书）进程级缓存：根证书装载要枚举系统证书库，
/// 每次击键都装一遍既慢又无意义；证书库内容变化极低频，进程生命周期
/// 内复用一份即可。
static TLS_CONNECTOR: OnceCell<TlsConnector> = OnceCell::new();

type TlsStream = tokio_rustls::client::TlsStream<TcpStream>;
type StreamKey = (u16, &'static str);

/// 单个 (端口, 主机) 的连接池：多条热连接 + 归还通知。
/// HTTP/1.1 连接同刻只能服务一个请求，打字并发的每个查询各占一条；
/// notify 在连接归还时唤醒池空等待者，避免其直接落冷建连。
struct HostPool {
    conns: Vec<TlsStream>,
    notify: Arc<tokio::sync::Notify>,
}

/// 长连接池表：键 (SOCKS 端口, suggest 主机)。端口入键是因为服务器
/// 重连后会话可能换端口，旧端口连接对新会话无意义。
static STREAMS: Lazy<AsyncMutex<HashMap<StreamKey, HostPool>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

struct BreakerState {
    consecutive_failures: u32,
    /// 冷却截止时刻；None = 未熔断
    cooldown_until: Option<Instant>,
}

/// 熔断按 (服务器, 引擎) 分键：不同出口的可达性互不相同（实测泰国出口
/// Google 黑洞、Bing 正常），且用户在多服务器间频繁切换——若只按引擎
/// 共享，A 服务器的失败会把 B 服务器正常引擎的联想误熔断 30s。
static BREAKERS: Lazy<AsyncMutex<HashMap<(String, u8), BreakerState>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

fn engine_key(engine: SearchEngine) -> u8 {
    match engine {
        SearchEngine::Google => 0,
        SearchEngine::Bing => 1,
    }
}

/// 引擎名（日志用，诊断"哪个引擎在哪个出口不通"）
fn engine_name(engine: SearchEngine) -> &'static str {
    match engine {
        SearchEngine::Google => "google",
        SearchEngine::Bing => "bing",
    }
}

/// 引擎是否处于熔断冷却期
async fn breaker_open(key: &(String, u8)) -> bool {
    BREAKERS
        .lock()
        .await
        .get(key)
        .and_then(|s| s.cooldown_until)
        .map(|until| Instant::now() < until)
        .unwrap_or(false)
}

/// 记一次失败；达阈值则进入冷却并返回 true
async fn note_failure(key: (String, u8)) -> bool {
    let mut map = BREAKERS.lock().await;
    let state = map.entry(key).or_insert(BreakerState {
        consecutive_failures: 0,
        cooldown_until: None,
    });
    state.consecutive_failures += 1;
    if state.consecutive_failures >= BREAK_THRESHOLD {
        state.cooldown_until = Some(Instant::now() + BREAK_COOLDOWN);
        state.consecutive_failures = 0;
        return true;
    }
    false
}

/// 记一次成功：清空该服务器该引擎的失败计数与熔断状态
async fn note_success(key: &(String, u8)) {
    BREAKERS.lock().await.remove(key);
}

/// 结果缓存 TTL：联想词时效性强，超期视为无缓存
const CACHE_TTL: Duration = Duration::from_secs(300);
/// 缓存容量上限（超出淘汰最旧）
const CACHE_MAX: usize = 64;

/// 联想结果缓存：同 (服务器, 引擎, 查询词) 的重复输入直接命中、不再走
/// 隧道（Chrome Omnibox 同款思路——前缀反复出现时秒出，也顺带省掉重复
/// 请求对隧道的占用）。仅缓存成功响应；失败/超时不缓存，避免放大故障。
static SUGGEST_CACHE: Lazy<AsyncMutex<HashMap<(String, u8, String), (Instant, Vec<String>)>>> =
    Lazy::new(|| AsyncMutex::new(HashMap::new()));

/// 读缓存：命中且未过期返回词列表
async fn cache_get(key: &(String, u8, String)) -> Option<Vec<String>> {
    let map = SUGGEST_CACHE.lock().await;
    map.get(key)
        .filter(|(at, _)| at.elapsed() < CACHE_TTL)
        .map(|(_, list)| list.clone())
}

/// 前缀扩展命中（Chrome Omnibox 同款技巧）：当前查询 q 的完整缓存 miss 时，
/// 若缓存中存在 q 的前缀查询 p（如输入 "rustl" 而缓存有 "rust"），由于联想词
/// 以查询开头，p 的结果里以 q 开头的项对 q 依然有效——过滤即得合法子集，0ms
/// 返回。取最长前缀缓存（过滤结果通常最多）；过滤为空则返回 None 走网络。
async fn cache_get_prefix(key: &(String, u8, String)) -> Option<Vec<String>> {
    let map = SUGGEST_CACHE.lock().await;
    let q = &key.2;
    let (_, (_, list)) = map
        .iter()
        .filter(|(k, (at, _))| {
            k.0 == key.0 && k.1 == key.1 && at.elapsed() < CACHE_TTL && q.starts_with(&k.2)
        })
        .max_by_key(|(_, (at, _))| *at)?;
    let filtered: Vec<String> = list.iter().filter(|s| s.starts_with(q)).cloned().collect();
    if filtered.is_empty() { None } else { Some(filtered) }
}

/// 写缓存：容量超限淘汰最旧条目
async fn cache_put(key: (String, u8, String), list: Vec<String>) {
    let mut map = SUGGEST_CACHE.lock().await;
    if map.len() >= CACHE_MAX {
        if let Some(oldest) = map.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) {
            map.remove(&oldest);
        }
    }
    map.insert(key, (Instant::now(), list));
}

/// 浏览器联想词：地址栏输入 → 搜索引擎补全词列表（失败返回空）
///
/// 引擎取自浏览器设置（与导航搜索同源）；port 为该 server 会话的
/// SOCKS5 监听端口，无活跃会话（断线挂起/未开浏览器）直接返回空。
#[tauri::command]
pub async fn browser_suggest(
    server_id: String,
    engine: SearchEngine,
    query: String,
    app: AppHandle,
) -> Vec<String> {
    let q = query.trim();
    if q.is_empty() {
        return vec![];
    }
    // State 借用不跨 await：先取出端口再进入网络流程
    let port = {
        let manager = app.state::<ProxySessionManager>();
        manager.socks_port(&server_id)
    };
    match port {
        Some(port) => fetch_suggestions(&server_id, port, engine, q).await,
        None => vec![],
    }
}

/// 经隧道拉取联想词（顶层入口：熔断检查 + 结果缓存 + 超时/失败统一静默降级为空列表）
pub(crate) async fn fetch_suggestions(
    server_id: &str,
    port: u16,
    engine: SearchEngine,
    query: &str,
) -> Vec<String> {
    let key = (server_id.to_string(), engine_key(engine));
    let name = engine_name(engine);
    if breaker_open(&key).await {
        tracing::debug!("[Suggest] {name} 熔断冷却中，跳过本次请求");
        return vec![];
    }
    // 缓存命中：同前缀刚查过，直接秒出（不占隧道）
    let cache_key = (key.0.clone(), key.1, query.to_string());
    if let Some(list) = cache_get(&cache_key).await {
        tracing::debug!("[Suggest] {name} 缓存命中（{query}）");
        return list;
    }
    // 前缀扩展命中：输入是已缓存查询的延伸时，从其结果过滤即得（0ms）
    if let Some(list) = cache_get_prefix(&cache_key).await {
        tracing::debug!("[Suggest] {name} 前缀缓存命中（{query}）");
        return list;
    }
    // 无请求级取消（历史教训：曾用 spawn + abort 取消在途旧请求，abort
    // 会把该 task 持有的池连接一并 drop——http_get 已从池中取出、尚未
    // 归还，预热连接被逐字杀光、每次查询退化为新建，即"预热从不命中"
    // 的根因）。过期查询不做任何后端取消：自然跑完（≤超时），结果由
    // 前端 seq 丢弃，连接完整归还池中；打字并发由连接池深度 + 池空短等
    // 归还消化（见 POOL_DEPTH / POOL_WAIT）。
    // 无外层 timeout：外层超时触发会 drop 整个 fetch_inner，把其持有的
    // 池连接一并泄漏（实测把池清空）。总时长由内部超时保证：
    // 建连段 timeout(FETCH_TIMEOUT) + 每次读 timeout(READ_TIMEOUT)
    let started = Instant::now();
    let result = fetch_inner(port, engine, query).await;
    match &result {
        Ok(list) => {
            note_success(&key).await;
            cache_put(cache_key, list.clone()).await;
            // INFO 级：生产日志可见用时，便于验证预热/复用效果（热连接应 <500ms）
            tracing::info!("[Suggest] {name} 成功，用时 {}ms", started.elapsed().as_millis());
        }
        Err(e) => {
            let tripped = note_failure(key).await;
            if tripped {
                tracing::info!(
                    "[Suggest] {name} 连续失败达 {BREAK_THRESHOLD} 次，熔断 {BREAK_COOLDOWN:?}（疑似 Agent 出口无法连通该引擎）"
                );
            }
            tracing::info!("[Suggest] {name} 联想获取失败（{}ms）: {e}", started.elapsed().as_millis());
        }
    }
    result.unwrap_or_default()
}

async fn fetch_inner(port: u16, engine: SearchEngine, query: &str) -> Result<Vec<String>, String> {
    let (host, prefix) = suggest_endpoint(engine);
    let path = format!("{prefix}{}", percent_encode(query));
    let body = http_get(port, host, &path).await?;
    Ok(parse_suggest_body(&body))
}

/// 引擎 →（suggest 主机, 查询路径前缀）。
/// Google client=firefox 与 Bing osjson 均返回 [词, [联想…]] 二元 JSON。
fn suggest_endpoint(engine: SearchEngine) -> (&'static str, &'static str) {
    match engine {
        SearchEngine::Google => (
            "suggestqueries.google.com",
            "/complete/search?client=firefox&q=",
        ),
        SearchEngine::Bing => ("api.bing.com", "/osjson.aspx?query="),
    }
}

/// 解析联想响应体：`[query, [suggestion…]]` 的第二项，非字符串项丢弃。
/// 结构异常/非 JSON 一律空列表（静默降级语义）。
fn parse_suggest_body(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return vec![];
    };
    v.get(1)
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 发送 GET 并读完整响应，优先复用池中热连接；失效换下一条重试，
/// 池空短等归还，仍无则冷建连兜底。返回 (响应体, 连接是否可归还缓存)。
async fn http_get(port: u16, host: &'static str, path: &str) -> Result<String, String> {
    let key: StreamKey = (port, host);
    // 热连接路径：首轮池空短等（打字突发时旧请求 ≈1 RTT 内归还）；
    // 复用失效（服务器闲置关流等）换池中下一条，不再等待直接冷建连
    for wait in [true, false] {
        let Some(mut stream) = take_stream(key, wait).await else {
            break;
        };
        let t0 = Instant::now();
        match request_on_stream(&mut stream, host, path).await {
            Ok((body, reusable)) => {
                // INFO：热连接命中时 HTTP 往回应仅 1 个跨洋 RTT（数百 ms 内）；
                // 若此处仍 >1s，说明瓶颈在隧道 RTT 或引擎服务器本身
                tracing::info!(
                    "[Suggest] {host} 复用热连接，HTTP 往回 {}ms",
                    t0.elapsed().as_millis()
                );
                if reusable {
                    put_stream(key, stream).await;
                }
                return Ok(body);
            }
            Err(e) => {
                // 失效连接已取走，此处 drop 关闭，换池中下一条重试。
                // INFO：诊断期必须可见——若预热后首查仍走重建，说明热连接
                // 提前死亡（QUIC 重连未清池/键不匹配/被对端关闭）
                tracing::info!("[Suggest] 复用连接失效，重建: {e}");
            }
        }
    }
    // 冷建连段包超时：此刻连接不在池中，超时 drop 无泄漏风险
    let mut stream = tokio::time::timeout(FETCH_TIMEOUT, connect_tls_via_socks(port, host))
        .await
        .map_err(|_| format!("{host} 建连超时（≥{FETCH_TIMEOUT:?}）"))??;
    let t0 = Instant::now();
    let (body, reusable) = request_on_stream(&mut stream, host, path).await?;
    tracing::info!(
        "[Suggest] {host} 新建连接后 HTTP 往回 {}ms",
        t0.elapsed().as_millis()
    );
    if reusable {
        put_stream(key, stream).await;
    }
    Ok(body)
}

/// 从池中取一条热连接。wait=true 时池空先短等在途请求归还（打字突发
/// 场景，见 POOL_WAIT 注释）；wait=false 只取现成连接（失效重试场景
/// 不等待，尽快落到冷建连）。
async fn take_stream(key: StreamKey, wait: bool) -> Option<TlsStream> {
    let mut map = STREAMS.lock().await;
    let Some(pool) = map.get_mut(&key) else {
        return None;
    };
    if let Some(stream) = pool.conns.pop() {
        return Some(stream);
    }
    if !wait {
        return None;
    }
    // 池空：挂起等待归还信号（锁不跨 await；put_stream 归还时 notify_one）
    let notify = pool.notify.clone();
    drop(map);
    if tokio::time::timeout(POOL_WAIT, notify.notified()).await.is_err() {
        tracing::debug!("[Suggest] 池空等待归还超时（{POOL_WAIT:?}），转冷建连");
        return None;
    }
    let mut map = STREAMS.lock().await;
    map.get_mut(&key).and_then(|p| p.conns.pop())
}

/// 连接归还池（超 POOL_DEPTH 丢弃多余，防御并发归还超发）
async fn put_stream(key: StreamKey, stream: TlsStream) {
    let mut map = STREAMS.lock().await;
    let pool = map.entry(key).or_insert_with(|| HostPool {
        conns: Vec::new(),
        notify: Arc::new(tokio::sync::Notify::new()),
    });
    if pool.conns.len() >= POOL_DEPTH {
        return; // 多余连接直接 drop 关闭
    }
    pool.conns.push(stream);
    pool.notify.notify_one();
}

/// 清空指定端口下的全部池连接（重连恢复路径专用）：QUIC 连接对象已更换，
/// 旧端口下连接全部随旧流死亡，残留只会让复用首查必失败一次。
pub(crate) async fn clear_streams_for_port(port: u16) {
    STREAMS.lock().await.retain(|(p, _), _| *p != port);
}

/// 会话启动/恢复后预热两个引擎的 suggest 连接（后台执行，失败静默）。
///
/// 首次联想查询若冷启动要付"QUIC 建流 + SOCKS 握手 + TLS 握手"全量开销
/// （实测 1.5-2.5s）；预热把建连挪到会话启动时刻，首次查询直接从连接池
/// 取热连接（~0.2s，1 个跨洋 RTT）。会话启动后地址栏输入前有几秒空窗，
/// 预热正好填满——Chrome 对默认搜索引擎同样做连接预热的同款思路。
///
/// 每主机预热 POOL_DEPTH 条且并发建连：HTTP/1.1 连接同刻只能服务一个
/// 请求，快速输入的并发查询各需一条热连接（见 POOL_DEPTH 注释）。
pub(crate) async fn warm_up_connections(port: u16) {
    let hosts: [&'static str; 2] = [
        suggest_endpoint(SearchEngine::Google).0,
        suggest_endpoint(SearchEngine::Bing).0,
    ];
    let mut tasks = Vec::new();
    for host in hosts {
        // 池中已有什么就补什么（恢复路径可能残留部分存活连接）
        let need = {
            let map = STREAMS.lock().await;
            POOL_DEPTH.saturating_sub(
                map.get(&(port, host)).map_or(0, |p| p.conns.len()),
            )
        };
        for _ in 0..need {
            tasks.push(tokio::spawn(async move {
                match connect_tls_via_socks(port, host).await {
                    Ok(stream) => {
                        // INFO 级：生产日志可见，确认预热生效
                        tracing::info!("[Suggest] 预热连接完成: {host}");
                        put_stream((port, host), stream).await;
                    }
                    Err(e) => tracing::debug!("[Suggest] 预热连接失败（不影响使用）: {host}: {e}"),
                }
            }));
        }
    }
    for t in tasks {
        let _ = t.await;
    }
}

/// 经本机 SOCKS5 监听建立到 host:443 的 TLS 连接。
/// 隧道内完成远程 DNS（CONNECT 透传域名）。
/// 三段计时（INFO）：定位延迟到底在隧道 CONNECT（QUIC 建流+Agent 拨号）、
/// TLS 握手还是本地，都只有 1-2 个跨洋 RTT，数值应各在数百 ms 内。
async fn connect_tls_via_socks(port: u16, host: &'static str) -> Result<TlsStream, String> {
    let t0 = Instant::now();
    let mut tcp = TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| format!("连本地代理端口: {e}"))?;
    let t1 = Instant::now();
    socks5_client_connect(&mut tcp, host, 443).await?;
    let t2 = Instant::now();
    let connector = tls_connector()?;
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|e| format!("ServerName 构造失败: {e}"))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("TLS 握手: {e}"))?;
    let t3 = Instant::now();
    tracing::info!(
        "[Suggest] 建连 {host}: 本地 {}ms | 隧道CONNECT {}ms | TLS {}ms",
        (t1 - t0).as_millis(),
        (t2 - t1).as_millis(),
        (t3 - t2).as_millis()
    );
    Ok(tls)
}

/// 在既有连接上发 GET 并读完整响应。泛型参数使响应定界逻辑可用
/// 内存双工流单测。返回 (响应体, 连接是否可复用)：
/// - Content-Length / chunked 定界 → 可复用
/// - 服务器声明 close / 无定界（靠关流） → 不可复用
async fn request_on_stream<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    host: &str,
    path: &str,
) -> Result<(String, bool), String> {
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Mozilla/5.0\r\nAccept: */*\r\nConnection: keep-alive\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| format!("写请求: {e}"))?;
    stream
        .flush()
        .await
        .map_err(|e| format!("flush 请求: {e}"))?;

    // 头部：\r\n\r\n 定界，超限视为异常服务器
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEAD {
            return Err("HTTP 头部超限".to_string());
        }
        read_more(stream, &mut buf).await?;
    };
    let head = parse_head(&String::from_utf8_lossy(&buf[..head_end]))?;

    let body_start = head_end + 4;
    let mut reusable = !head.server_closes;
    let body = if head.chunked {
        let mut pos = body_start;
        read_chunked(stream, &mut buf, &mut pos).await?
    } else if let Some(len) = head.content_length {
        let end = body_start + len as usize;
        while buf.len() < end {
            read_more(stream, &mut buf).await?;
        }
        // 缓冲里出现超过 Content-Length 的字节属于流错位（不该发生），
        // 保守起见丢弃连接不复用
        if buf.len() > end {
            reusable = false;
        }
        buf[body_start..end].to_vec()
    } else {
        // 无定界：读至对端关流。对端不发 close_notify 直接断开属常规收尾
        // （HTTP/1.0 同款语义），读错误也按已收数据处理
        reusable = false;
        let mut tmp = [0u8; 4096];
        loop {
            // 同 read_more：per-read 超时（无定界响应读至关流）。
            // 超时/读错/EOF 都按关流收尾（已收数据够解析则可用）
            let n = match tokio::time::timeout(READ_TIMEOUT, stream.read(&mut tmp)).await {
                Ok(Ok(n)) => n,
                Ok(Err(_)) => break,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.len() > MAX_BODY {
                return Err("响应体超限".to_string());
            }
        }
        buf[body_start..].to_vec()
    };
    Ok((String::from_utf8_lossy(&body).into_owned(), reusable))
}

struct RespHead {
    content_length: Option<u64>,
    chunked: bool,
    /// 服务器声明 Connection: close（响应后关流，不可复用）
    server_closes: bool,
}

/// 解析响应头：状态行必须 200；提取定界与关流声明。
fn parse_head(head: &str) -> Result<RespHead, String> {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("空状态行")?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or("HTTP 状态行异常")?;
    if status != "200" {
        return Err(format!("HTTP {status}"));
    }
    let mut h = RespHead {
        content_length: None,
        chunked: false,
        server_closes: false,
    };
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => h.content_length = value.trim().parse().ok(),
            "transfer-encoding" => h.chunked = value.to_ascii_lowercase().contains("chunked"),
            "connection" => h.server_closes = value.to_ascii_lowercase().contains("close"),
            _ => {}
        }
    }
    Ok(h)
}

/// 续读一批数据；对端关流（EOF）视为协议错误——length/chunked 定界下
/// 提前 EOF 即响应不完整。
async fn read_more<S: AsyncRead + Unpin>(stream: &mut S, buf: &mut Vec<u8>) -> Result<(), String> {
    let mut tmp = [0u8; 4096];
    // per-read 超时（3s）：正常单次读 ≤1 个跨洋 RTT（数百 ms）。读超时即连接
    // 已死/挂死，drop 重建。**不能用外层 timeout 包整个请求**——外层超时触发
    // 会 drop 整个 future，已从池中取出的连接随之泄漏（实测把池清空，预热
    // 全部白费）；per-read 只在真死连接上丢弃，正常路径连接完整归还
    let n = tokio::time::timeout(READ_TIMEOUT, stream.read(&mut tmp))
        .await
        .map_err(|_| "读响应超时（3s/次）".to_string())?
        .map_err(|e| format!("读响应: {e}"))?;
    if n == 0 {
        return Err("对端提前关闭连接".to_string());
    }
    buf.extend_from_slice(&tmp[..n]);
    if buf.len() > MAX_BODY {
        return Err("响应体超限".to_string());
    }
    Ok(())
}

/// 从 buf 游标处取一行（\r\n 定界，行不含终止符），需续读时经 stream 拉取。
/// 返回行在 buf 中的下标区间，游标推进到行尾之后。
async fn ensure_line<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut Vec<u8>,
    pos: &mut usize,
) -> Result<(usize, usize), String> {
    loop {
        if let Some(rel) = buf[*pos..].windows(2).position(|w| w == b"\r\n") {
            let start = *pos;
            let end = start + rel;
            *pos = end + 2;
            return Ok((start, end));
        }
        read_more(stream, buf).await?;
    }
}

/// chunked 解码（RFC 9112 §7.1）：行读十六进制大小 → 读块体+CRLF →
/// 终块 0 → 尾随头逐行读到空行。
async fn read_chunked<S: AsyncRead + Unpin>(
    stream: &mut S,
    buf: &mut Vec<u8>,
    pos: &mut usize,
) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let (start, end) = ensure_line(stream, buf, pos).await?;
        let size_str = std::str::from_utf8(&buf[start..end]).map_err(|_| "chunk 大小非 UTF-8")?;
        // 大小行允许 ";ext=…" 扩展，取分号前主体
        let size_hex = size_str.trim().split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size_hex, 16).map_err(|_| format!("chunk 大小非法: {size_hex}"))?;
        if size == 0 {
            loop {
                let (ts, te) = ensure_line(stream, buf, pos).await?;
                if ts == te {
                    break; // 空行：尾随头结束
                }
            }
            return Ok(body);
        }
        if body.len() + size > MAX_BODY {
            return Err("响应体超限".to_string());
        }
        let chunk_start = *pos;
        let chunk_end = chunk_start + size;
        while buf.len() < chunk_end + 2 {
            read_more(stream, buf).await?;
        }
        body.extend_from_slice(&buf[chunk_start..chunk_end]);
        if &buf[chunk_end..chunk_end + 2] != b"\r\n" {
            return Err("chunk 缺少 CRLF".to_string());
        }
        *pos = chunk_end + 2;
    }
}

/// TLS 连接器（懒构建）：OS 根证书 → rustls 根库 → 默认校验器。
/// 构建失败仅影响联想功能（返回空列表），不影响浏览主链路。
fn tls_connector() -> Result<&'static TlsConnector, String> {
    TLS_CONNECTOR.get_or_try_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        // 系统证书库（Windows 走 schannel cert store）；个别证书解析失败
        // 属正常（跨平台库差异），add_parsable_certificates 跳过即可
        let native = rustls_native_certs::load_native_certs();
        for e in &native.errors {
            tracing::debug!("[Suggest] 系统证书读取告警: {e}");
        }
        let (added, ignored) = roots.add_parsable_certificates(native.certs);
        tracing::debug!("[Suggest] 根证书装载: {added} 可用, {ignored} 跳过");
        if added == 0 {
            return Err("系统根证书装载失败".to_string());
        }
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok::<_, String>(TlsConnector::from(Arc::new(config)))
    })
}

/// SOCKS5 客户端握手（RFC 1928 子集，与 socks5.rs 服务端实现对端）：
/// greeting（仅无认证）+ CONNECT 域名目标（透传域名 → 远程 DNS）。
async fn socks5_client_connect(tcp: &mut TcpStream, host: &str, port: u16) -> Result<(), String> {
    // greeting：VER=5, NMETHODS=1, METHOD=无认证；回复必须选无认证
    tcp.write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| format!("SOCKS5 写 greeting: {e}"))?;
    let mut ack = [0u8; 2];
    tcp.read_exact(&mut ack)
        .await
        .map_err(|e| format!("SOCKS5 读 greeting 回复: {e}"))?;
    if ack[0] != 0x05 {
        return Err(format!("SOCKS5 版本异常: {}", ack[0]));
    }
    if ack[1] != 0x00 {
        return Err(format!("代理不接受无认证方式（0x{:02X}）", ack[1]));
    }

    // CONNECT：VER=5 CMD=CONNECT RSV ATYP=域名 LEN 域名 端口
    let mut req = Vec::with_capacity(host.len() + 7);
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host.len() as u8]);
    req.extend_from_slice(host.as_bytes());
    req.extend_from_slice(&port.to_be_bytes());
    tcp.write_all(&req)
        .await
        .map_err(|e| format!("SOCKS5 写 CONNECT: {e}"))?;

    // 回复：VER REP RSV ATYP BND.ADDR BND.PORT——只关心 REP，按 ATYP 跳过地址
    let mut head = [0u8; 4];
    tcp.read_exact(&mut head)
        .await
        .map_err(|e| format!("SOCKS5 读 CONNECT 回复: {e}"))?;
    if head[1] != 0x00 {
        return Err(format!("代理拒绝连接（REP=0x{:02X}）", head[1]));
    }
    let addr_len = match head[3] {
        0x01 => 4, // IPv4
        0x03 => {
            let mut len = [0u8; 1];
            tcp.read_exact(&mut len)
                .await
                .map_err(|e| format!("SOCKS5 读地址长度: {e}"))?;
            len[0] as usize
        }
        0x04 => 16, // IPv6
        other => return Err(format!("未知地址类型 0x{other:02X}")),
    };
    let mut rest = vec![0u8; addr_len + 2]; // BND.ADDR + BND.PORT
    tcp.read_exact(&mut rest)
        .await
        .map_err(|e| format!("SOCKS5 读绑定地址: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::socks5::{socks5_greeting, socks5_read_connect, socks5_reply};

    #[test]
    fn 解析联想json_两元结构第二项() {
        let body = r#"["rust",["rust","rust lang","rustle",["rich object"]]]"#;
        assert_eq!(parse_suggest_body(body), vec!["rust", "rust lang", "rustle"]);
    }

    #[test]
    fn 解析联想json_异常一律空() {
        assert!(parse_suggest_body("").is_empty());
        assert!(parse_suggest_body("not json").is_empty());
        // 结构不对：第一项是对象而非数组
        assert!(parse_suggest_body(r#"["q", {"a":1}]"#).is_empty());
        // 缺第二项
        assert!(parse_suggest_body(r#"["q"]"#).is_empty());
    }

    #[test]
    fn 响应头解析_定界与关流声明() {
        let h = parse_head("HTTP/1.1 200 OK\r\nContent-Length: 21\r\nContent-Type: application/json").unwrap();
        assert_eq!(h.content_length, Some(21));
        assert!(!h.chunked && !h.server_closes);

        let h = parse_head("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked").unwrap();
        assert!(h.chunked && h.content_length.is_none());

        let h = parse_head("HTTP/1.1 200 OK\r\nConnection: close").unwrap();
        assert!(h.server_closes);

        assert!(parse_head("HTTP/1.1 302 Found").is_err());
        assert!(parse_head("").is_err());
    }

    #[tokio::test]
    async fn 响应定界_content_length_连接可复用() {
        let body = r#"["rust",["rust lang"]]"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (mut client, mut server) = tokio::io::duplex(8192);
        let req_task = tokio::spawn(async move {
            let mut req = vec![0u8; 4096];
            let n = server.read(&mut req).await.unwrap();
            server.write_all(resp.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&req[..n]).into_owned()
        });
        let (got, reusable) = request_on_stream(
            &mut client,
            "suggestqueries.google.com",
            "/complete/search?client=firefox&q=rust",
        )
        .await
        .unwrap();
        let req = req_task.await.unwrap();
        assert!(req.starts_with("GET /complete/search?client=firefox&q=rust HTTP/1.1\r\n"));
        assert!(req.contains("Connection: keep-alive"));
        assert_eq!(got, body);
        assert!(reusable);
    }

    #[tokio::test]
    async fn 响应定界_chunked_跨块续读() {
        // 响应体拆两个 chunk 且分块边界落在 UTF-8 无关的任意位置
        let body = r#"["rust",["rust lang"]]"#;
        let (a, b) = body.split_at(5);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
            a.len(),
            b.len()
        );
        let (mut client, mut server) = tokio::io::duplex(8192);
        let req_task = tokio::spawn(async move {
            let mut req = vec![0u8; 4096];
            let n = server.read(&mut req).await.unwrap();
            server.write_all(resp.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&req[..n]).into_owned()
        });
        let (got, reusable) = request_on_stream(
            &mut client,
            "suggestqueries.google.com",
            "/complete/search?client=firefox&q=rust",
        )
        .await
        .unwrap();
        req_task.await.unwrap();
        assert_eq!(got, body);
        assert!(reusable);
    }

    #[tokio::test]
    async fn 响应定界_服务器声明close_不可复用() {
        let body = r#"["rust",["rust lang"]]"#;
        let (mut client, mut server) = tokio::io::duplex(8192);
        let req_task = tokio::spawn(async move {
            let mut req = vec![0u8; 4096];
            let n = server.read(&mut req).await.unwrap();
            server
                .write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n{body}").as_bytes())
                .await
                .unwrap();
            // 不关流：验证定界靠 Connection: close 声明而非 EOF——
            // 无定界分支才会读至 EOF；此处声明 close + 无定界，
            // 实现按 EOF 语义读。关流让读循环退出。
            server.shutdown().await.unwrap();
            String::from_utf8_lossy(&req[..n]).into_owned()
        });
        let (got, reusable) = request_on_stream(
            &mut client,
            "api.bing.com",
            "/osjson.aspx?query=rust",
        )
        .await
        .unwrap();
        req_task.await.unwrap();
        assert_eq!(got, body);
        assert!(!reusable);
    }

    #[tokio::test]
    async fn 响应定界_length不足时提前关流_报错() {
        let (mut client, mut server) = tokio::io::duplex(8192);
        tokio::spawn(async move {
            let mut req = vec![0u8; 4096];
            let _ = server.read(&mut req).await.unwrap();
            // 声明 100 字节只给 3 字节然后关流
            server
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nabc")
                .await
                .unwrap();
            server.shutdown().await.unwrap();
        });
        let err = request_on_stream(&mut client, "api.bing.com", "/x").await.unwrap_err();
        assert!(err.contains("提前关闭"), "实际错误: {err}");
    }

    #[tokio::test]
    async fn 客户端握手_与服务端实现互通() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut b, _) = listener.accept().await.unwrap();
            socks5_greeting(&mut b).await.unwrap();
            let target = socks5_read_connect(&mut b).await.unwrap();
            assert_eq!(target.host, "suggestqueries.google.com");
            assert_eq!(target.port, 443);
            socks5_reply(&mut b, 0x00).await.unwrap();
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        socks5_client_connect(&mut client, "suggestqueries.google.com", 443)
            .await
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn 客户端握手_代理拒绝连接报错() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut b, _) = listener.accept().await.unwrap();
            socks5_greeting(&mut b).await.unwrap();
            socks5_read_connect(&mut b).await.unwrap();
            // general failure（0x01）：Agent 连不上目标
            socks5_reply(&mut b, 0x01).await.unwrap();
        });
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let err = socks5_client_connect(&mut client, "example.com", 443)
            .await
            .unwrap_err();
        assert!(err.contains("REP=0x01"), "实际错误: {err}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn 缓存_命中过期与容量淘汰() {
        let key = ("测试缓存".to_string(), 0u8, "rust".to_string());
        SUGGEST_CACHE.lock().await.remove(&key);
        assert!(cache_get(&key).await.is_none(), "无缓存");
        cache_put(key.clone(), vec!["rust lang".into()]).await;
        assert_eq!(
            cache_get(&key).await,
            Some(vec!["rust lang".to_string()]),
            "TTL 内应命中"
        );
        // 模拟过期
        SUGGEST_CACHE.lock().await.get_mut(&key).unwrap().0 =
            Instant::now() - CACHE_TTL - Duration::from_secs(1);
        assert!(cache_get(&key).await.is_none(), "过期应视为无缓存");
        // 容量淘汰：填满后最旧者被逐出
        SUGGEST_CACHE.lock().await.remove(&key);
        for i in 0..CACHE_MAX {
            cache_put(("测试缓存".into(), 1u8, format!("q{i}")), vec![]).await;
        }
        cache_put(("测试缓存".into(), 1u8, "first".into()), vec![]).await;
        cache_put(("测试缓存".into(), 1u8, "last".into()), vec![]).await;
        let map = SUGGEST_CACHE.lock().await;
        assert!(map.len() <= CACHE_MAX, "容量不应超限");
        // "first"/"last" 比 q 系列新，应存活；被逐出的是最旧的 q 条目
        assert!(map.contains_key(&("测试缓存".to_string(), 1u8, "first".to_string())));
        assert!(map.contains_key(&("测试缓存".to_string(), 1u8, "last".to_string())));
        let q_remaining = (0..CACHE_MAX)
            .filter(|i| map.contains_key(&("测试缓存".to_string(), 1u8, format!("q{i}"))))
            .count();
        assert_eq!(q_remaining, CACHE_MAX - 2, "恰好淘汰两个最旧的 q 条目");
        drop(map);
        SUGGEST_CACHE.lock().await.clear();
    }

    #[tokio::test]
    async fn 熔断_连续失败触发冷却_成功复位() {
        // 测试专用键（服务器+引擎元组），不与其他测试/引擎冲突
        let key = ("测试服务器".to_string(), 77u8);
        BREAKERS.lock().await.remove(&key);
        assert!(!breaker_open(&key).await);
        assert!(!note_failure(key.clone()).await, "首次失败不触发");
        assert!(note_failure(key.clone()).await, "达阈值应触发熔断");
        assert!(breaker_open(&key).await, "冷却期内应视为打开");
        // 模拟冷却过期
        BREAKERS.lock().await.get_mut(&key).unwrap().cooldown_until =
            Some(Instant::now() - Duration::from_secs(1));
        assert!(!breaker_open(&key).await, "冷却过期应放行试探");
        note_success(&key).await;
        assert!(!breaker_open(&key).await);
        BREAKERS.lock().await.remove(&key);
    }

    /// 需外网（直连 api.bing.com）：验证长连接两连发均成功且可复用。
    /// 手动运行：cargo test --lib -- --ignored suggest
    #[tokio::test]
    #[ignore = "需要外网直连 api.bing.com"]
    async fn 直连bing_长连接复用两连发() {
        // 测试进程不经过应用 setup，需自行安装 rustls crypto provider
        // （沿用 connection.rs 的既有模式；已安装时 Err 可忽略）
        let _ = rustls::crypto::ring::default_provider().install_default();
        let connector = tls_connector().unwrap().clone();
        let tcp = TcpStream::connect("api.bing.com:443").await.unwrap();
        let name =
            rustls::pki_types::ServerName::try_from("api.bing.com".to_string()).unwrap();
        let mut stream = connector.connect(name, tcp).await.unwrap();
        let (body1, reusable1) =
            request_on_stream(&mut stream, "api.bing.com", "/osjson.aspx?query=rust")
                .await
                .unwrap();
        assert!(reusable1, "第一发应可复用（若 false 说明服务器未做长度定界）");
        assert!(!parse_suggest_body(&body1).is_empty(), "应解析出联想词");
        let (_body2, reusable2) =
            request_on_stream(&mut stream, "api.bing.com", "/osjson.aspx?query=rustl")
                .await
                .unwrap();
        assert!(reusable2, "第二发在同一连接上应成功且可继续复用");
    }

    /// 需本机 11080 有活的 SOCKS 会话（应用正在运行）：经隧道跑完整
    /// fetch 链路，复现/隔离"curl 通、应用超时"的差异。
    /// 手动运行：cargo test --lib -- --ignored suggest
    #[tokio::test]
    #[ignore = "需要本机 127.0.0.1:11080 活跃会话"]
    async fn 隧道复现_经socks拉bing联想() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let port: u16 = std::env::var("SUGGEST_TEST_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(11080);
        let started = Instant::now();
        let mut stream = connect_tls_via_socks(port, "api.bing.com")
            .await
            .expect("TLS 经 SOCKS 建连");
        tracing::info!("建连用时 {}ms", started.elapsed().as_millis());
        let (body, reusable) = request_on_stream(&mut stream, "api.bing.com", "/osjson.aspx?query=rust")
            .await
            .expect("请求+读响应");
        tracing::info!(
            "请求用时 {}ms, body={} 字节, reusable={reusable}",
            started.elapsed().as_millis(),
            body.len()
        );
        assert!(!parse_suggest_body(&body).is_empty(), "应解析出联想词");
    }
}
