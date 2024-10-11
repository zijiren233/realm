pub mod cmd;
pub mod conf;
pub mod consts;
use conf::{EndpointConf, NetConf};
pub use realm_core as core;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ENV_CONFIG: &str = "REALM_CONF";

use std::ffi::CStr;
use std::os::raw::c_char;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};
use crate::conf::{Config, LogConf, DnsConf, EndpointInfo};

use once_cell::sync::Lazy;
use std::net::TcpListener;

// 全局运行时映射，用于管理多个Realm实例
static RUNTIME_MAP: Lazy<Arc<Mutex<HashMap<String, (tokio::runtime::Runtime, usize, String)>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

// 日志初始化标志
static LOG_INIT: Once = Once::new();

// DNS初始化标志
static DNS_INIT: Once = Once::new();

/// 在C语言中使用Realm库的方法:
///
/// 1. 包含头文件:
///    #include "realm.h"
///
/// 2. 调用start_realm函数:
///    const char* listen_addr = start_realm("remote", "host", "path", true, false);
///
/// 3. 关闭服务:
///    stop_realm("remote", "host", "path", true, false);
///
/// 注意:
/// - 确保已经正确编译并链接了Realm库
/// - start_realm函数不再阻塞，而是在后台运行
#[no_mangle]
pub extern "C" fn start_realm(
    remote: *const c_char,
    host: *const c_char,
    path: *const c_char,
    tls: bool,
    insecure: bool,
) -> *const c_char {
    // 初始化日志和DNS（仅执行一次）
    initialize_once();

    // 将C字符串转换为Rust字符串
    let (remote, host, path) = convert_cstr_to_str(remote, host, path);

    // 创建唯一的配置键
    let config_key = format!("{}-{}-{}-{}-{}", remote, host, path, tls, insecure);
    let mut runtime_map = RUNTIME_MAP.lock().expect("Failed to lock RUNTIME_MAP");

    // 检查是否已存在相同配置的实例
    if let Some((_, count, listen_addr)) = runtime_map.get_mut(&config_key) {
        *count += 1;
        return std::ffi::CString::new(listen_addr.clone()).unwrap().into_raw();
    }

    // 创建网络配置
    let net = create_net_conf();

    // 绑定到本地随机端口
    let listen_addr = bind_to_random_port();

    // 创建端点配置
    let endpoint = create_endpoint_conf(remote, listen_addr.clone(), net, path, tls, insecure);

    // 构建端点信息
    let endpoints = build_endpoints(endpoint);

    // 创建运行时并启动服务
    let runtime = create_runtime();
    runtime.spawn(run(endpoints));

    // 将新的运行时实例添加到映射中
    runtime_map.insert(config_key, (runtime, 1, listen_addr.clone()));
    std::ffi::CString::new(listen_addr).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn stop_realm(
    remote: *const c_char,
    host: *const c_char,
    path: *const c_char,
    tls: bool,
    insecure: bool,
) {
    // 将C字符串转换为Rust字符串
    let (remote, host, path) = convert_cstr_to_str(remote, host, path);

    // 创建唯一的配置键
    let config_key = format!("{}-{}-{}-{}-{}", remote, host, path, tls, insecure);
    let mut runtime_map = RUNTIME_MAP.lock().expect("Failed to lock RUNTIME_MAP");

    // 检查并更新实例计数
    if let Some((_, count, _)) = runtime_map.get_mut(&config_key) {
        *count -= 1;
        if *count == 0 {
            // 如果计数为0，移除并关闭运行时
            if let Some((runtime, _, _)) = runtime_map.remove(&config_key) {
                runtime.shutdown_background();
                log::info!("Realm instance with config {} has been stopped", config_key);
            }
        }
    } else {
        log::warn!("No Realm instance found with config {}", config_key);
    }
}

/// 初始化日志和DNS（仅执行一次）
fn initialize_once() {
    LOG_INIT.call_once(|| setup_log(LogConf::default()));
    DNS_INIT.call_once(|| setup_dns(DnsConf::default()));
}

/// 将C字符串转换为Rust字符串
fn convert_cstr_to_str(
    remote: *const c_char,
    host: *const c_char,
    path: *const c_char,
) -> (&'static str, &'static str, &'static str) {
    unsafe {
        (
            CStr::from_ptr(remote).to_str().expect("Invalid remote string"),
            CStr::from_ptr(host).to_str().expect("Invalid host string"),
            CStr::from_ptr(path).to_str().expect("Invalid path string"),
        )
    }
}

/// 创建网络配置
fn create_net_conf() -> NetConf {
    let mut net = NetConf::default();
    net.use_udp = Some(true);
    net.no_tcp = Some(false);
    net
}

/// 绑定到本地随机端口
fn bind_to_random_port() -> String {
    let localhost = "127.0.0.1";
    let listener = TcpListener::bind(format!("{}:0", localhost)).expect("Failed to bind to a random port");
    let port = listener.local_addr().expect("Failed to get local address").port();
    drop(listener);
    format!("{}:{}", localhost, port)
}

/// 创建端点配置
fn create_endpoint_conf(
    remote: &str,
    listen_addr: String,
    net: NetConf,
    path: &str,
    tls: bool,
    insecure: bool,
) -> EndpointConf {
    let remote_transport = if tls {
        if insecure {
            format!("ws;host={};path={};tls;sni={};insecure", remote, path, remote)
        } else {
            format!("ws;host={};path={};tls;sni={}", remote, path, remote)
        }
    } else {
        format!("ws;host={};path={}", remote, path)
    };

    EndpointConf {
        listen: listen_addr,
        remote: remote.to_string(),
        extra_remotes: vec![],
        balance: None,
        through: None,
        interface: None,
        listen_transport: None,
        remote_transport: Some(remote_transport),
        network: net,
    }
}

/// 构建端点信息
fn build_endpoints(endpoint: EndpointConf) -> Vec<EndpointInfo> {
    vec![endpoint]
        .into_iter()
        .map(Config::build)
        .inspect(|x| log::info!("Initialized: {}", x.endpoint))
        .collect()
}

/// 设置日志
fn setup_log(log: LogConf) {
    log::info!("Setting up log: {}", &log);

    let (level, output) = log.build();
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}]{}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(level)
        .chain(output)
        .apply()
        .expect("Failed to setup logger")
}

/// 设置DNS
fn setup_dns(dns: DnsConf) {
    log::info!("Setting up DNS: {}", &dns);

    let (conf, opts) = dns.build();
    core::dns::build_lazy(conf, opts);
}

/// 创建Tokio运行时
fn create_runtime() -> tokio::runtime::Runtime {
    #[cfg(feature = "multi-thread")]
    {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build multi-thread runtime")
    }

    #[cfg(not(feature = "multi-thread"))]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to build current-thread runtime")
    }
}

/// 运行Realm服务
async fn run(endpoints: Vec<EndpointInfo>) {
    use crate::core::tcp::run_tcp;
    use crate::core::udp::run_udp;
    use futures::future::join_all;

    let workers = endpoints
        .into_iter()
        .flat_map(
            |EndpointInfo {
                 endpoint,
                 no_tcp,
                 use_udp,
             }| {
                let mut tasks = Vec::with_capacity(2);
                if use_udp {
                    tasks.push(tokio::spawn(run_udp(endpoint.clone())));
                }
                if !no_tcp {
                    tasks.push(tokio::spawn(run_tcp(endpoint)));
                }
                tasks
            },
        )
        .collect::<Vec<_>>();

    join_all(workers).await;
}
