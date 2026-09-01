//! `McpHub` — mọi server bên thứ ba, từ một đối tượng.
//!
//! Ba tính chất, và cả ba đều là quyết định:
//!
//! **Best-effort.** Một server không chạy, chạy rồi treo, hay trả về một danh sách tool
//! vô nghĩa đều dừng lại trong task giám sát của chính nó. Không có đường nào từ đó ra tới
//! `bash` hay `read` của người dùng. Bản Python nói điều này bằng một `try/except` quanh
//! chỗ nối (`mcp/client.py:216-218`); ở đây nó là hình dạng của cấu trúc: mỗi server là
//! một task, và một task chết chỉ làm một task chết.
//!
//! **Một chủ cho một kết nối.** [`rmcp::service::RunningService`] không bao giờ rời khỏi
//! task giám sát. Mọi thứ khác cầm [`rmcp::service::Peer`] — xem ghi chú đầu crate về chỗ
//! bản Rust không cần chép cấu trúc né tránh của bản Python.
//!
//! **Hot-reload.** Thêm hay bớt một server không cần khởi động lại ứng dụng, và quan
//! trọng hơn: nó **không đụng tới những server không đổi**. Bản Python bắt phải restart,
//! nghĩa là thêm một server thứ tám làm rụng bảy kết nối đang khoẻ. Xem [`McpHub::reload`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pai_core::Guard;
use pai_tools::{Resolution, ToolRegistry};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::{oneshot, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{CONNECT_TIMEOUT, ConfigError, DEFAULT_MAX_RETRIES, ServerConfig};
use crate::dial::{ConfigDialers, Dialer, DialerFactory, Reach};
use crate::naming::qualify;
use crate::remote::{Link, RemoteTool};

/// Một kết nối sống được chừng này rồi mới đứt thì lần đứt đó là sự cố, không phải một
/// server hỏng ngay từ đầu — nên bộ đếm số lần thử được đặt lại.
///
/// Không có ngưỡng này thì một server chết ngay sau `initialize` sẽ được nối lại vô hạn:
/// mỗi lần nối thành công đặt bộ đếm về không, và vòng lặp quay tít mà không bao giờ chạm
/// tới giới hạn số lần thử.
const HEALTHY_AFTER: Duration = Duration::from_secs(30);

/// Trạng thái một server, đủ để giao diện vẽ ra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerState {
    Connecting,
    Ready {
        tools: usize,
    },
    /// Đã hết số lần thử. Server vẫn còn trong danh sách để người dùng thấy nó hỏng.
    Failed {
        reason: String,
    },
    Stopped,
}

/// Một server, đủ để giao diện vẽ một hàng và trả lời được câu hỏi của người dùng.
///
/// Ba trường sau cùng đều là những thứ [`McpHub::state`] một mình không nói được, và đều là
/// thứ người dùng hỏi đầu tiên khi có gì đó không chạy: *cắm được bao nhiêu tool, tên gì,
/// và nếu hỏng thì hỏng vì cái gì.*
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerStatus {
    pub name: String,
    pub state: ServerState,
    /// Tên **đầy đủ đã mang tiền tố**: `ext.<server>.<tool>`.
    ///
    /// Chỗ này dễ sai đúng một cách: hub biết tên từ xa, nhưng cái người dùng phải thấy là
    /// cái mô hình thật sự gọi. Hiện tên trần thì người dùng đi tìm một tool không tồn tại
    /// trong sổ đăng ký, và không ai giải thích được vì sao.
    pub tools: Vec<String>,
    /// Lý do **lần hỏng gần nhất**, giữ lại kể cả sau khi đã nối lại được.
    ///
    /// Một server chập chờn nối lại thành công rồi lại đứt sẽ không để lại dấu vết nào nếu
    /// ta xoá lý do mỗi lần nối được; người dùng chỉ thấy một danh sách tool lúc có lúc
    /// không.
    pub error: Option<String>,
}

/// Kết quả của một lần cắm server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mount {
    Connected {
        tools: usize,
    },
    /// Không nối được **lần đầu**. Không phải lỗi: task giám sát vẫn đang thử lại, và
    /// người dùng vẫn còn đủ tool của mình.
    Unavailable {
        reason: String,
    },
}

/// Bao lâu và bao nhiêu lần.
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy {
    pub connect_timeout: Duration,
    pub max_retries: u32,
}

impl Default for RetryPolicy {
    fn default() -> RetryPolicy {
        RetryPolicy {
            connect_timeout: CONNECT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

/// Chỗ task giám sát ghi lại những gì [`McpHub::status`] đọc ra.
///
/// Không dùng thêm một `watch` nữa vì đây không phải thứ ai đó chờ đợi trên đó — nó chỉ
/// được đọc lúc giao diện vẽ lại, và một ô có khoá là thứ rẻ nhất làm được việc đó.
#[derive(Default)]
struct Report {
    /// Đã mang tiền tố, đúng như trong sổ đăng ký.
    tools: Mutex<Vec<String>>,
    error: Mutex<Option<String>>,
}

struct Mounted {
    /// Ảnh chụp cấu hình, để [`McpHub::reload`] biết cái gì đổi. `None` cho server cắm
    /// bằng [`McpHub::mount_dialer`] — không có cấu hình thì không so được, nên coi như
    /// luôn khác và người gọi tự quản.
    fingerprint: Option<String>,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
    state: watch::Receiver<ServerState>,
    report: Arc<Report>,
}

/// Mọi server bên thứ ba mà ứng dụng đang nói chuyện.
pub struct McpHub {
    registry: Arc<ToolRegistry>,
    servers: Mutex<HashMap<String, Mounted>>,
    dialers: Arc<dyn DialerFactory>,
}

impl McpHub {
    pub fn new(registry: Arc<ToolRegistry>) -> Arc<McpHub> {
        McpHub::with_dialers(registry, Arc::new(ConfigDialers))
    }

    /// Cùng cái hub, khác cách mở kết nối. Xem [`DialerFactory`].
    pub fn with_dialers(
        registry: Arc<ToolRegistry>,
        dialers: Arc<dyn DialerFactory>,
    ) -> Arc<McpHub> {
        Arc::new(McpHub {
            registry,
            servers: Mutex::new(HashMap::new()),
            dialers,
        })
    }

    /// Cắm một server theo cấu hình người dùng khai.
    ///
    /// `Err` chỉ dành cho cấu hình sai — thứ người dùng sửa được. Một server không nối
    /// được là `Ok(Mount::Unavailable)`: đó là một sự việc, không phải một lỗi của lời
    /// gọi, và gấp nó thành `Err` sẽ cám dỗ chỗ gọi làm đúng cái việc bị cấm — để một
    /// server bên thứ ba làm hỏng lượt khởi động.
    pub async fn mount(&self, config: ServerConfig) -> Result<Mount, ConfigError> {
        config.validate()?;
        let policy = RetryPolicy {
            connect_timeout: config.connect_timeout(),
            max_retries: config.max_retries,
        };
        let name = config.name.clone();
        let fingerprint = fingerprint(&config);
        let dialer = self.dialers.make(&config);
        Ok(self.install(name, dialer, policy, Some(fingerprint)).await)
    }

    /// Cắm một server bằng một [`Dialer`] tự dựng. Đây là cửa mà bài kiểm chứng đi vào.
    pub async fn mount_dialer(
        &self,
        name: impl Into<String>,
        dialer: Arc<dyn Dialer>,
        policy: RetryPolicy,
    ) -> Mount {
        self.install(name.into(), dialer, policy, None).await
    }

    async fn install(
        &self,
        name: String,
        dialer: Arc<dyn Dialer>,
        policy: RetryPolicy,
        fingerprint: Option<String>,
    ) -> Mount {
        // Cắm đè lên một cái tên đang có là thay thế, không phải thêm: hai task giám sát
        // cùng một tên sẽ tranh nhau đăng ký cùng một bộ tool.
        self.unmount(&name).await;

        let cancel = CancellationToken::new();
        let (state_tx, state_rx) = watch::channel(ServerState::Connecting);
        let (first_tx, first_rx) = oneshot::channel();
        let link = Link::new();
        let report = Arc::new(Report::default());

        let supervisor = Supervisor {
            name: name.clone(),
            dialer,
            registry: self.registry.clone(),
            link,
            state: state_tx,
            report: report.clone(),
            ct: cancel.clone(),
            policy,
        };
        let handle = tokio::spawn(supervisor.run(Some(first_tx)));

        self.servers.lock().insert(
            name.clone(),
            Mounted {
                fingerprint,
                cancel,
                handle,
                state: state_rx,
                report,
            },
        );

        // Chờ **lần thử đầu tiên** ngã ngũ, không chờ hết mọi lần thử lại. Người dùng cần
        // biết ngay bây giờ mình có tool nào; những lần thử sau chạy trong nền.
        //
        // Vẫn có một hạn chót ở đây dù `dial` đã có hạn chót của nó: một `Dialer` do người
        // khác viết không bắt buộc phải tôn trọng cái nào cả, và treo cả lượt khởi động vì
        // một server bên thứ ba là đúng thứ crate này tồn tại để tránh.
        let deadline = policy.connect_timeout.saturating_mul(2) + Duration::from_secs(1);
        match tokio::time::timeout(deadline, first_rx).await {
            Ok(Ok(mount)) => mount,
            Ok(Err(_)) => Mount::Unavailable {
                reason: "task giám sát dừng trước khi báo kết quả".into(),
            },
            Err(_) => Mount::Unavailable {
                reason: format!("`{name}` không trả lời trong {deadline:?}"),
            },
        }
    }

    /// Gỡ một server: đóng kết nối và **gỡ mọi tool của nó khỏi sổ đăng ký**.
    ///
    /// Việc gỡ tool không nằm ở đây mà nằm trong task giám sát: guard của lượt đăng ký là
    /// biến cục bộ của nó, nên chỉ cần task đó kết thúc là sổ sạch. Không có bảng nào phải
    /// dọn bằng tay, và không có đường nào để quên.
    pub async fn unmount(&self, name: &str) -> bool {
        let Some(mounted) = self.servers.lock().remove(name) else {
            return false;
        };
        mounted.cancel.cancel();
        if let Err(err) = mounted.handle.await {
            tracing::warn!(server = %name, %err, "task giám sát MCP kết thúc bất thường");
        }
        true
    }

    /// Đặt lại toàn bộ danh sách server, **không đụng vào cái không đổi**.
    ///
    /// Đây là điểm khác thật sự so với bản Python, nơi mọi thay đổi đòi khởi động lại ứng
    /// dụng. Một server có cấu hình y hệt trước đó giữ nguyên kết nối, nguyên phiên, và
    /// nguyên bộ tool đã đăng ký; chỉ cái được thêm mới bị nối, chỉ cái bị bỏ mới bị đóng.
    ///
    /// Trả về kết quả cho từng cái **được đụng tới**, theo thứ tự trong `configs`.
    pub async fn reload(
        &self,
        configs: Vec<ServerConfig>,
    ) -> Vec<(String, Result<Mount, ConfigError>)> {
        let mut wanted: HashMap<String, ServerConfig> = HashMap::new();
        let mut report: Vec<(String, Result<Mount, ConfigError>)> = Vec::new();

        for config in configs {
            if let Err(err) = config.validate() {
                report.push((config.name.clone(), Err(err)));
                continue;
            }
            if !config.enabled {
                continue;
            }
            wanted.insert(config.name.clone(), config);
        }

        let obsolete: Vec<String> = {
            let servers = self.servers.lock();
            servers
                .iter()
                .filter(|(name, mounted)| match wanted.get(*name) {
                    None => true,
                    Some(config) => mounted.fingerprint.as_deref() != Some(&fingerprint(config)),
                })
                .map(|(name, _)| name.clone())
                .collect()
        };
        for name in obsolete {
            self.unmount(&name).await;
        }

        for (name, config) in wanted {
            if self.servers.lock().contains_key(&name) {
                continue;
            }
            report.push((name, self.mount(config).await));
        }
        report
    }

    /// Đóng tất cả. Dành cho lúc gỡ plugin.
    pub async fn shutdown(&self) {
        let names: Vec<String> = self.servers.lock().keys().cloned().collect();
        for name in names {
            self.unmount(&name).await;
        }
    }

    pub fn servers(&self) -> Vec<String> {
        let mut names: Vec<String> = self.servers.lock().keys().cloned().collect();
        names.sort();
        names
    }

    pub fn state(&self, name: &str) -> Option<ServerState> {
        Some(self.servers.lock().get(name)?.state.borrow().clone())
    }

    /// Ảnh chụp mọi server đang cắm, xếp theo tên để giao diện không nhảy hàng giữa hai
    /// lần vẽ.
    ///
    /// Chỉ những server **đang cắm**: một server bị tắt không có kết nối, không có tool, và
    /// không có gì để nói ở đây — trạng thái "tắt" là chuyện của kho cấu hình, xem
    /// [`crate::store`].
    pub fn status(&self) -> Vec<ServerStatus> {
        let mut out: Vec<ServerStatus> = {
            let servers = self.servers.lock();
            servers
                .iter()
                .map(|(name, mounted)| ServerStatus {
                    name: name.clone(),
                    state: mounted.state.borrow().clone(),
                    tools: mounted.report.tools.lock().clone(),
                    error: mounted.report.error.lock().clone(),
                })
                .collect()
        };
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

fn fingerprint(config: &ServerConfig) -> String {
    // Cấu hình luôn serialize được — nó là struct của chính ta. Nhánh hỏng chỉ làm dấu vân
    // tay rỗng, tức là lần reload sau nối lại server đó; đắt hơn một chút, không sai.
    serde_json::to_string(config).unwrap_or_default()
}

/// Đăng ký một danh sách tool từ xa, trả về guard của từng cái **và tên đã mang tiền tố**.
///
/// Thả `Vec<Guard>` là gỡ sạch. Đó là lý do nó được trả ra chứ không được cất vào trong.
/// Tên trả về cùng chỗ với guard, vì đó là chỗ duy nhất biết cái nào **thật sự** vào được
/// sổ — hai cái bị bỏ qua ở dưới không được phép hiện ra trong danh sách người dùng đọc.
fn register_tools(
    registry: &Arc<ToolRegistry>,
    server: &str,
    tools: &[rmcp::model::Tool],
    link: &Arc<Link>,
    reach: Reach,
) -> (Vec<Guard>, Vec<String>) {
    let mut guards = Vec::new();
    let mut names = Vec::new();
    for tool in tools {
        // Tiền tố được đặt **ở đây** — chỗ đầu tiên cái tên từ xa chạm vào hệ thống của
        // ta, trước sổ đăng ký, trước log, trước mọi con mắt.
        let name = qualify(server, &tool.name);

        if !name.round_trips() {
            tracing::warn!(
                server = %server, tool = %tool.name,
                "bỏ qua: tên chứa `__` nên không mã hoá sang dạng wire một cách khả nghịch"
            );
            continue;
        }

        // Tiền tố đã bảo đảm không đụng tool nội bộ. Lần kiểm này canh cái còn lại: một
        // server tên `a` công bố tool `b.c` và một server tên `a.b` công bố tool `c` sẽ ra
        // cùng một danh tính. Cấu hình đã cấm dấu chấm trong tên server nên chuyện đó
        // không xảy ra được — nhưng bất biến "đăng ký sau che đăng ký trước" là thứ đắt
        // đến mức đáng kiểm hai lần, ở hai chỗ độc lập.
        if !matches!(
            registry.resolve(None, name.as_str()),
            Resolution::Unknown(_)
        ) {
            tracing::warn!(tool = %name, "bỏ qua: đã có tool cùng tên trong sổ đăng ký");
            continue;
        }

        let parameters = Value::Object((*tool.input_schema).clone());
        names.push(name.as_str().to_string());
        guards.push(registry.register(Arc::new(RemoteTool::new(
            name,
            tool.name.to_string(),
            server,
            tool.description.clone().unwrap_or_default().to_string(),
            parameters,
            link.clone(),
            reach,
        ))));
    }
    (guards, names)
}

fn backoff(attempt: u32) -> Duration {
    // 1s, 2s, 4s, 8s, 16s rồi dừng ở đó. Có chặn trên là bắt buộc: cấp số nhân không chặn
    // thì lần thử thứ mười rơi vào giữa đêm hôm sau.
    Duration::from_secs(1u64 << (attempt.clamp(1, 5) - 1))
}

/// Task sở hữu kết nối tới đúng một server.
///
/// Gom tham số vào một struct chứ không rải ra tám đối số, và không phải để cho gọn: vòng
/// đời ở đây là *nối → đăng ký → chờ chết → gỡ → nối lại*, và mỗi trường dưới đây là một
/// thứ sống qua trọn vòng đó. Cái duy nhất không sống qua nổi một vòng là `first`, nên nó
/// là đối số của [`Supervisor::run`] chứ không phải một trường.
struct Supervisor {
    name: String,
    dialer: Arc<dyn Dialer>,
    registry: Arc<ToolRegistry>,
    link: Arc<Link>,
    state: watch::Sender<ServerState>,
    report: Arc<Report>,
    ct: CancellationToken,
    policy: RetryPolicy,
}

impl Supervisor {
    async fn run(self, mut first: Option<oneshot::Sender<Mount>>) {
        let mut attempt: u32 = 0;

        loop {
            if self.ct.is_cancelled() {
                break;
            }
            let _ = self.state.send(ServerState::Connecting);

            // Token con: huỷ nó đóng đúng kết nối này, và lần nối lại sau có token của
            // riêng nó. Dùng chung một token cho cả vòng đời thì `RunningService` lúc bị
            // thả sẽ huỷ token đó — và lần nối lại kế tiếp bắt đầu bằng một token đã chết.
            let conn_ct = self.ct.child_token();
            let dialed = tokio::select! {
                () = self.ct.cancelled() => break,
                result = tokio::time::timeout(
                    self.policy.connect_timeout,
                    self.dialer.dial(conn_ct),
                ) => result,
            };

            let service = match dialed {
                Ok(Ok(service)) => service,
                Ok(Err(err)) => {
                    if !self.retry(&mut attempt, &mut first, err.to_string()).await {
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    let reason = format!(
                        "hết {:?} mà chưa xong initialize",
                        self.policy.connect_timeout
                    );
                    if !self.retry(&mut attempt, &mut first, reason).await {
                        break;
                    }
                    continue;
                }
            };

            let tools = match service.peer().list_all_tools().await {
                Ok(tools) => tools,
                Err(err) => {
                    // Kết nối mở nhưng vô dụng. Đóng nó lại trước khi thử lại, nếu không
                    // mỗi vòng để lại một tiến trình con còn sống.
                    let _ = service.cancel().await;
                    let reason = format!("không đọc được danh sách tool: {err}");
                    if !self.retry(&mut attempt, &mut first, reason).await {
                        break;
                    }
                    continue;
                }
            };

            self.link.set(service.peer().clone());
            let (guards, names) = register_tools(
                &self.registry,
                &self.name,
                &tools,
                &self.link,
                self.dialer.reach(),
            );
            let count = guards.len();
            *self.report.tools.lock() = names;
            tracing::info!(server = %self.name, tools = count, "MCP server đã sẵn sàng");
            let _ = self.state.send(ServerState::Ready { tools: count });
            settle(&mut first, Mount::Connected { tools: count });

            // Chuyển quyền sở hữu kết nối sang một task con để chờ nó chết mà không phải
            // tiêu thụ nó ở đây: `waiting()` nhận `self` theo giá trị, nên đây là cách duy
            // nhất vừa chờ được vừa còn `select!` được với lệnh dừng.
            let started = Instant::now();
            let mut watcher = tokio::spawn(async move { service.waiting().await });
            let stopped_by_us = tokio::select! {
                () = self.ct.cancelled() => true,
                _ = &mut watcher => false,
            };
            if stopped_by_us {
                // Đợi task sở hữu kết nối dọn xong. Không đợi thì tiến trình con của một
                // server stdio còn sống thêm một lúc sau khi người dùng đã bấm gỡ.
                let _ = watcher.await;
            }

            // Thả guard **trước** khi thử lại: trong lúc chưa có kết nối, không tool nào
            // của server này được phép còn nằm trong danh sách quảng cáo cho mô hình.
            drop(guards);
            self.link.clear();
            // Danh sách tool phải rỗng đúng lúc sổ đăng ký rỗng. Lệch nhau một nhịp là
            // giao diện mời người dùng gọi một tool vừa biến mất.
            self.report.tools.lock().clear();

            if self.ct.is_cancelled() {
                break;
            }
            // Sống đủ lâu thì lần đứt này là sự cố, không phải một server hỏng sẵn.
            if started.elapsed() >= HEALTHY_AFTER {
                attempt = 0;
            }
            if !self
                .retry(&mut attempt, &mut first, "kết nối đứt".into())
                .await
            {
                break;
            }
        }

        self.link.clear();
        // `Failed` không được ghi đè bằng `Stopped`. Hai chuyện đó khác nhau ở đúng thứ mà
        // người dùng cần biết: một cái là "tôi đã bỏ cuộc, đây là lý do", cái kia là "bạn
        // bảo tôi dừng". Ghi đè thì mọi server hỏng đều trông như vừa được gỡ đi tử tế.
        let gave_up = matches!(*self.state.borrow(), ServerState::Failed { .. });
        if !gave_up {
            let _ = self.state.send(ServerState::Stopped);
        }
    }

    /// `false` nghĩa là thôi, đừng thử nữa.
    async fn retry(
        &self,
        attempt: &mut u32,
        first: &mut Option<oneshot::Sender<Mount>>,
        reason: String,
    ) -> bool {
        tracing::warn!(server = %self.name, %reason, "MCP server không dùng được");
        // Ghi lý do ngay, và **không** xoá nó ở lần nối thành công sau: đây là câu trả lời
        // cho "vì sao lúc nãy nó hỏng", mà câu hỏi đó luôn được hỏi sau khi mọi thứ có vẻ
        // đã ổn trở lại.
        *self.report.error.lock() = Some(reason.clone());
        // Báo kết quả lần đầu **ngay bây giờ**, trước khi ngủ: chỗ gọi `mount` đang chờ, và
        // bắt nó chờ hết cả chuỗi thử lại là bắt cửa sổ ứng dụng chờ theo.
        settle(
            first,
            Mount::Unavailable {
                reason: reason.clone(),
            },
        );

        *attempt += 1;
        if *attempt > self.policy.max_retries {
            tracing::warn!(server = %self.name, "thôi không nối lại MCP server nữa");
            let _ = self.state.send(ServerState::Failed { reason });
            return false;
        }

        let delay = backoff(*attempt);
        tokio::select! {
            () = self.ct.cancelled() => false,
            () = tokio::time::sleep(delay) => true,
        }
    }
}

fn settle(first: &mut Option<oneshot::Sender<Mount>>, mount: Mount) {
    if let Some(tx) = first.take() {
        let _ = tx.send(mount);
    }
}
