//! Tệp cấu hình mà `pai-rag-service` đọc.
//!
//! # Vì sao là một tệp chứ không phải biến môi trường
//!
//! Người dùng đổi mô hình nhúng trong Cài đặt lúc service đang chạy là chuyện thường.
//! Biến môi trường được ấn định lúc `spawn` và không đổi được nữa, nên mỗi lần đổi một ô
//! trong Cài đặt sẽ phải giết tiến trình con và dựng lại — mất vài giây, và mất cả phiên
//! ONNX của reranker đang nạp sẵn.
//!
//! Một tệp thì service soi `mtime` và tự nạp lại ở lời gọi kế tiếp. Đổi mô hình có hiệu
//! lực ngay, không ai phải khởi động lại gì.
//!
//! # Ai là nguồn sự thật
//!
//! **Ứng dụng**, không phải service. Người dùng chọn provider và mô hình ở màn hình Cài
//! đặt; kho provider giữ lựa chọn đó; module này chiếu nó xuống tệp. Service không có
//! màn hình cấu hình nào và không được phép có — hai chỗ cùng nhớ một lựa chọn là hai
//! chỗ sẽ lệch nhau, và lúc ấy người dùng nhìn thấy một mô hình trong Cài đặt còn tài
//! liệu thì được nhúng bằng mô hình khác.
//!
//! Ngoại lệ đúng một nhóm: điểm cuối và mật khẩu của Qdrant với Neo4j đến từ
//! `services/rag/deploy/.env` — tệp mà `docker compose` cũng đọc. Chép chúng vào kho
//! provider là bắt người dùng khai cùng một mật khẩu ở hai nơi.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pai_llm::ProviderKind;
use pai_providers::{Role, StoredProvider};
use serde_json::{Value, json};

/// Bản của định dạng tệp. Service từ chối một bản nó không hiểu thay vì đoán.
const VERSION: u32 = 1;

/// Dự án đang mở, ở dạng service cần: mã ổn định, tên hiển thị, và thư mục.
///
/// Một struct nhỏ thay cho một bộ ba `(&str, &str, &Path)`: ba tham số cùng kiểu chuỗi
/// đứng cạnh nhau là chỗ để hoán vị hai cái mà trình biên dịch không bắt được.
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// Chỗ đặt tệp, và cách dựng nội dung của nó.
pub struct RagConfigFile {
    path: PathBuf,
    /// Thư mục dữ liệu của service — kho SQLite siêu dữ liệu của từng dự án.
    data_dir: PathBuf,
    /// `services/rag/deploy/.env`, nếu có.
    deploy_env: Option<PathBuf>,
}

impl RagConfigFile {
    pub fn new(app_data_dir: &Path, service_dir: &Path) -> RagConfigFile {
        // **Tuyệt đối hoá.** Tiến trình service chạy với `cwd` riêng của nó
        // (`services/rag`, để `uv` tìm thấy `pyproject.toml`), nên một đường dẫn tương
        // đối trong biến môi trường của nó trỏ vào một chỗ khác hẳn chỗ ta vừa ghi.
        //
        // Đây không phải phòng xa: `Config::from_env` lùi về `"."` khi không có `HOME`,
        // và trên Windows thì không có `HOME` — nên `data_dir` thật sự là tương đối ở
        // đường chạy thường gặp nhất. Triệu chứng là service báo không thấy tệp cấu hình
        // trong khi tệp nằm ngay đó, chỉ là "ngay đó" của hai tiến trình khác nhau.
        let base = absolute(app_data_dir);
        RagConfigFile {
            path: base.join("rag-config.json"),
            data_dir: base.join("rag"),
            deploy_env: Some(absolute(service_dir).join("deploy").join(".env")),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ghi lại tệp từ trạng thái provider hiện tại.
    ///
    /// Nuốt lỗi ghi thành một dòng log chứ không trả `Result` lên: chỗ gọi là đường áp
    /// lại provider, và một ổ đĩa đầy không được phép chặn người dùng đổi mô hình trò
    /// chuyện. Service sẽ chạy bằng tệp cũ, và `stats().reason` nói ra khi có gì lệch.
    pub fn write(&self, providers: &[StoredProvider], project: Option<Project>) {
        if let Err(err) = self.try_write(providers, project) {
            tracing::warn!(%err, path = %self.path.display(), "không ghi được cấu hình RAG");
        }
    }

    fn try_write(
        &self,
        providers: &[StoredProvider],
        project: Option<Project>,
    ) -> std::io::Result<()> {
        let body = serde_json::to_vec_pretty(&self.build(providers, project))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Ghi ra tệp tạm rồi đổi tên: service soi `mtime` và có thể đọc đúng lúc ta đang
        // ghi. Một lần đọc trúng nửa tệp là một lỗi JSON khó hiểu ở phía bên kia.
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, body)?;
        std::fs::rename(&temp, &self.path)
    }

    fn build(&self, providers: &[StoredProvider], project: Option<Project>) -> Value {
        let env = self.deploy_env();
        let get = |key: &str, fallback: &str| -> String {
            env.get(key).cloned().unwrap_or_else(|| fallback.to_string())
        };

        let projects = match &project {
            Some(Project { id, name, root }) => json!([{
                "id": id,
                "name": name,
                "root": root.display().to_string(),
            }]),
            None => json!([]),
        };

        json!({
            "version": VERSION,
            "data_dir": self.data_dir.display().to_string(),
            "projects": projects,
            "active_project": project.as_ref().map(|item| item.id.clone()).unwrap_or_default(),
            "embedding": provider_json(holder(providers, Role::Embedding), Role::Embedding),
            "vision": provider_json(holder(providers, Role::Vision), Role::Vision),
            "chat": provider_json(holder(providers, Role::Chat), Role::Chat),
            "vectors": {
                "url": format!("http://127.0.0.1:{}", get("QDRANT_HTTP_PORT", "6333")),
                "api_key": get("QDRANT_API_KEY", ""),
            },
            "graph": {
                // Bật khi và chỉ khi có mật khẩu: một Neo4j không đăng nhập được thì
                // chiến lược graph vắng mặt, và `auto` lùi về `hybrid`. Nói ra bằng một
                // cờ tắt còn hơn để service thử nối rồi hỏng ở mỗi câu hỏi.
                "enabled": !get("NEO4J_PASSWORD", "").is_empty(),
                "uri": format!("bolt://127.0.0.1:{}", get("NEO4J_BOLT_PORT", "7687")),
                "user": get("NEO4J_USER", "neo4j"),
                "password": get("NEO4J_PASSWORD", ""),
            },
            // Rerank và OCR để service tự quyết bằng mặc định của nó. Chúng không phải
            // lựa chọn về *provider*, nên nhét chúng vào đây là đưa một quyết định của
            // tầng RAG lên một tầng không biết gì về nó.
        })
    }

    /// Đọc `deploy/.env`. Rỗng khi chưa có tệp — người dùng chưa dựng Docker bao giờ.
    fn deploy_env(&self) -> BTreeMap<String, String> {
        let Some(path) = &self.deploy_env else {
            return BTreeMap::new();
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return BTreeMap::new();
        };
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| {
                let value = value.trim();
                // Bỏ nháy nếu người dùng gõ vào — `docker compose` cũng làm vậy.
                let value = value
                    .strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .unwrap_or(value);
                (key.trim().to_string(), value.to_string())
            })
            .collect()
    }
}

/// Đường dẫn tuyệt đối, ghép với thư mục hiện hành khi nó còn tương đối.
///
/// Không dùng `canonicalize`: nó đòi đường dẫn **phải tồn tại**, mà thư mục dữ liệu có
/// thể chưa được tạo ở lần chạy đầu. Nó cũng trả về dạng `\?\` trên Windows, thứ mà
/// một số công cụ xử lý không đúng.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(path),
        // Không đọc được thư mục hiện hành thì đường dẫn tương đối vẫn hơn không có gì:
        // lỗi phía service sẽ in ra chính nó, và đó là manh mối cần thiết.
        Err(_) => path.to_path_buf(),
    }
}

/// Provider đang giữ một vai.
fn holder(providers: &[StoredProvider], role: Role) -> Option<&StoredProvider> {
    providers
        .iter()
        .find(|provider| provider.holds(role) && provider.config.enabled)
}

/// Một provider, ở dạng service hiểu.
///
/// Provider vắng mặt hoặc chưa chọn mô hình thì `model` rỗng — và service coi chuỗi rỗng
/// là "vai này chưa dùng được". Đừng mượn `model` của vai hội thoại điền vào: `qwen3:8b`
/// không có endpoint embed và không nhìn được ảnh.
fn provider_json(provider: Option<&StoredProvider>, role: Role) -> Value {
    let Some(provider) = provider else {
        return json!({ "kind": "ollama", "base_url": "", "api_key": "", "model": "" });
    };
    let model = match role {
        Role::Chat => provider.model.clone(),
        Role::Embedding => provider.embedding_model.clone(),
        Role::Vision => provider.vision_model.clone(),
    };
    json!({
        // Service chỉ phân biệt hai giao thức. LM Studio nói giao thức OpenAI ở cả phần
        // nhúng lẫn phần chat-với-ảnh; khác biệt của nó nằm ở kho mô hình, chỗ mà service
        // không đụng tới.
        "kind": match provider.config.kind {
            ProviderKind::Ollama => "ollama",
            ProviderKind::LmStudio | ProviderKind::OpenAiCompatible => "openai",
        },
        "base_url": provider.config.base_url,
        "api_key": provider.config.api_key,
        "model": model.unwrap_or_default(),
    })
}
