//! Configuration read directly by the native Rust document library. The app is the source
//! of truth; infrastructure values may also come from the root Docker Compose `.env`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pai_llm::ProviderKind;
use pai_providers::{Role, StoredProvider};
use serde_json::{Value, json};

/// File format version; native RAG rejects an unknown one rather than guessing.
const VERSION: u32 = 1;

/// The open project as native RAG needs it: stable id, display name, directory. A struct rather than a
/// same-typed triple the compiler cannot keep in order.
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// Where the file lives, and how its contents are built.
pub struct RagConfigFile {
    path: PathBuf,
    /// The native library's data directory: the per-project SQLite metadata store.
    data_dir: PathBuf,
    /// Root Docker Compose `.env`, if present.
    deploy_env: Option<PathBuf>,
}

impl RagConfigFile {
    pub fn new(app_data_dir: &Path, deploy_env: Option<&Path>) -> RagConfigFile {
        let base = absolute(app_data_dir);
        RagConfigFile {
            path: base.join("rag-config.json"),
            data_dir: base.join("rag"),
            deploy_env: deploy_env.map(absolute),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `rerank` entry currently in the file, or `None` when untouched.
    pub fn rerank(&self) -> Option<Value> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        let parsed: Value = serde_json::from_str(&raw).ok()?;
        parsed
            .get("rerank")
            .cloned()
            .filter(|found| found.is_object())
    }

    /// The `chunk` entry currently in the file, or `None` when untouched. Like [`RagConfigFile::rerank`],
    /// this is a user setting the provider rewrite must carry across rather than reset.
    pub fn chunk(&self) -> Option<Value> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        let parsed: Value = serde_json::from_str(&raw).ok()?;
        parsed
            .get("chunk")
            .cloned()
            .filter(|found| found.is_object())
    }

    /// Rewrite the `chunk` entry, leaving everything else alone.
    pub fn write_chunk(&self, chunk: Value) -> std::io::Result<()> {
        let mut root: Value = match std::fs::read_to_string(&self.path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| json!({})),
            Err(_) => json!({}),
        };
        if !root.is_object() {
            root = json!({});
        }
        root["chunk"] = chunk;
        self.atomic(&serde_json::to_vec_pretty(&root)?)
    }

    /// OCR is on by default. Return the persisted switch and the current vision model so the library screen can
    /// explain whether enabling it is actionable instead of presenting a checkbox that can never succeed.
    pub fn ocr_status(&self) -> (bool, Option<String>) {
        let parsed = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .unwrap_or_else(|| json!({}));
        let enabled = parsed
            .pointer("/ocr/enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let vision = parsed
            .pointer("/vision/model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_owned);
        (enabled, vision)
    }

    /// Reading the pictures inside pages that already have text; off unless the user asked for it, since most
    /// illustrations hold nothing to index and each one is a model call.
    pub fn ocr_images(&self) -> bool {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|parsed| parsed.pointer("/ocr/images").and_then(Value::as_bool))
            .unwrap_or(false)
    }

    pub fn write_ocr_enabled(&self, enabled: bool) -> std::io::Result<()> {
        self.write_ocr_flag("enabled", enabled)
    }

    pub fn write_ocr_images(&self, enabled: bool) -> std::io::Result<()> {
        self.write_ocr_flag("images", enabled)
    }

    /// Read-modify-write of one `ocr` flag, leaving the rest of the file alone: everything else in it is
    /// derived from the provider store, which this type does not hold.
    fn write_ocr_flag(&self, key: &str, enabled: bool) -> std::io::Result<()> {
        let mut root: Value = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        if !root.get("ocr").is_some_and(Value::is_object) {
            root["ocr"] = json!({});
        }
        root["ocr"][key] = json!(enabled);
        self.atomic(&serde_json::to_vec_pretty(&root)?)
    }

    /// Rewrite the `rerank` entry, leaving everything else alone: read-modify-write, because the rest derives
    /// from the provider store we do not hold here.
    pub fn write_rerank(&self, rerank: Value) -> std::io::Result<()> {
        let mut root: Value = match std::fs::read_to_string(&self.path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_else(|_| json!({})),
            // A missing file is normal on a fresh install: settings can be opened before any provider is applied.
            Err(_) => json!({}),
        };
        if !root.is_object() {
            root = json!({});
        }
        root["rerank"] = rerank;
        self.atomic(&serde_json::to_vec_pretty(&root)?)
    }

    /// Rewrite the file from current provider state; write errors become a log line rather than a `Result`,
    /// so a full disk cannot block switching chat models.
    pub fn write(&self, providers: &[StoredProvider], project: Option<Project>) {
        if let Err(err) = self.try_write(providers, project) {
            tracing::warn!(%err, path = %self.path.display(), "could not write the RAG configuration");
        }
    }

    fn try_write(
        &self,
        providers: &[StoredProvider],
        project: Option<Project>,
    ) -> std::io::Result<()> {
        self.atomic(&serde_json::to_vec_pretty(&self.build(providers, project))?)
    }

    /// Write the whole file via a rename: native RAG watches `mtime` and may read mid-write, where half a file
    /// becomes a baffling JSON error on the other side.
    fn atomic(&self, body: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, body)?;
        std::fs::rename(&temp, &self.path)
    }

    fn build(&self, providers: &[StoredProvider], project: Option<Project>) -> Value {
        let mut root = self.build_inner(providers, project);
        // Preserve the user's choice across rewrites, and insert only when present: an absent key means
        // "unset, use service defaults", while `null` is a type error the service rejects.
        if let Some(rerank) = self.rerank() {
            root["rerank"] = rerank;
        }
        // Same rule for chunking: switching chat model must not silently re-cut and re-embed the library.
        if let Some(chunk) = self.chunk() {
            root["chunk"] = chunk;
        }
        // Provider changes rewrite this file; preserve a user's OCR choice through those unrelated updates.
        if let Ok(raw) = std::fs::read_to_string(&self.path)
            && let Ok(previous) = serde_json::from_str::<Value>(&raw)
            && let Some(ocr) = previous.get("ocr").filter(|value| value.is_object())
        {
            root["ocr"] = ocr.clone();
        }
        root
    }

    fn build_inner(&self, providers: &[StoredProvider], project: Option<Project>) -> Value {
        let env = self.deploy_env();
        let get = |key: &str, fallback: &str| -> String {
            env.get(key)
                .cloned()
                .or_else(|| std::env::var(key).ok())
                .unwrap_or_else(|| fallback.to_string())
        };
        let graph_url = get(
            "PAI_RAG_GRAPH_URL",
            &format!("ws://127.0.0.1:{}", get("SURREAL_HTTP_PORT", "8000")),
        );

        let projects = match &project {
            Some(Project { id, name, root }) => json!([{
                "id": id,
                "name": name,
                "root": strip_verbatim(root).display().to_string(),
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
                "enabled": true,
                // The desktop app owns this loopback SurrealDB sidecar in production.
                "url": graph_url,
                "namespace": "pai",
            },
        })
    }

    /// Read the root Compose `.env`; empty when the file is absent.
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
                // Strip quotes if the user typed them, as `docker compose` does.
                let value = value
                    .strip_prefix('"')
                    .and_then(|rest| rest.strip_suffix('"'))
                    .unwrap_or(value);
                (key.trim().to_string(), value.to_string())
            })
            .collect()
    }
}

/// An absolute path, joined with the current directory when relative; not `canonicalize`, which requires the
/// path to exist and yields Windows verbatim form.
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return strip_verbatim(path);
    }
    match std::env::current_dir() {
        Ok(cwd) => strip_verbatim(&cwd.join(path)),
        // If the cwd is unreadable, a relative path beats nothing: the service's error will print it, which is the clue.
        Err(_) => path.to_path_buf(),
    }
}

/// Strip Windows verbatim path prefixes, which `canonicalize` produces and which reach the document table and
/// citations where users cannot recognise their own folder. The cost is >260-character paths without
/// `LongPathsEnabled`, which is the right trade for a document library.
fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    // Handle the UNC form first: it starts with the same four characters, and the branch below would mangle it.
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path.to_path_buf(),
    }
}

/// The provider holding a given role.
fn holder(providers: &[StoredProvider], role: Role) -> Option<&StoredProvider> {
    providers
        .iter()
        .find(|provider| provider.holds(role) && provider.config.enabled)
}

/// One provider in the shape native RAG understands; an absent provider or unset model yields an empty
/// `model`, meaning that role is unavailable. Never borrow the chat model here.
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
        // The service distinguishes only two protocols; LM Studio speaks OpenAI for both embedding and vision chat.
        "kind": match provider.config.kind {
            ProviderKind::Ollama => "ollama",
            ProviderKind::LmStudio | ProviderKind::OpenAiCompatible => "openai",
        },
        "base_url": provider.config.base_url,
        "api_key": provider.config.api_key,
        "model": model.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A provider save rewrites this whole file. The user's own settings -- chunking here -- must survive
    /// that, or picking a different chat model would silently re-cut and re-embed the entire library.
    #[test]
    fn a_provider_rewrite_keeps_the_chunking_the_user_chose() {
        let temp = tempfile::tempdir().expect("thư mục tạm");
        let config = RagConfigFile::new(temp.path(), None);

        config
            .write_chunk(json!({ "size": 600, "overlap": 40 }))
            .expect("ghi cấu hình cắt đoạn");
        assert_eq!(config.chunk(), Some(json!({ "size": 600, "overlap": 40 })));

        // No providers and no open project: the emptiest possible rewrite, which is the worst case here.
        config.write(&[], None);

        assert_eq!(
            config.chunk(),
            Some(json!({ "size": 600, "overlap": 40 })),
            "cấu hình cắt đoạn bị mất khi ghi lại tệp"
        );
    }

    /// Nothing written means nothing claimed: the service then uses its own defaults, and an empty `chunk`
    /// object in the file would be a different statement from an absent one.
    #[test]
    fn an_untouched_file_claims_no_chunking() {
        let temp = tempfile::tempdir().expect("thư mục tạm");
        let config = RagConfigFile::new(temp.path(), None);
        assert_eq!(config.chunk(), None);
        config.write(&[], None);
        assert_eq!(config.chunk(), None);
    }
}
