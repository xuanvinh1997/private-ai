//! What a model can do, from two sources in this order: Ollama's `/api/show`, which is
//! authoritative because it reads the GGUF, and only then a guess from the name.
//! Name guessing is inference over a string someone else chose; it is the last resort.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The capability set Ollama reports; anything outside it is dropped as newer Ollama vocabulary.
pub const OLLAMA_CAPABILITIES: [&str; 5] = ["chat", "embedding", "vision", "tools", "thinking"];

/// Ollama calls plain text generation "completion"; the rest of the app calls it chat.
const OLLAMA_ALIASES: [(&str, &str); 1] = [("completion", "chat")];

/// Substrings that mark a vision model; copied verbatim from `capabilities.py` - accumulated experience, not reasoning, so do not "tidy" it.
const VISION_TOKENS: [&str; 11] = [
    "-vl",
    ":vl",
    "clip",
    "gemma3",
    "gpt-4o",
    "gpt-5",
    "llava",
    "minicpm-v",
    "moondream",
    "o4-mini",
    "vision",
];

/// Where the capability came from; "this model cannot call tools" reads very differently as a fact read from a file than as a guess from a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    /// Declared by the server.
    Reported,
    /// Guessed from the model name.
    Inferred,
}

/// What a model can do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub chat: bool,
    pub embedding: bool,
    /// Can see images.
    pub vision: bool,
    /// Can call tools. The agent loop reads this to decide whether to hand over tool schemas.
    pub tools: bool,
    /// Has a separate reasoning channel.
    pub thinking: bool,
    /// Context window in tokens; `None` means unknown, not unlimited, so the caller picks a default.
    pub context_window: Option<u64>,
    pub source: CapabilitySource,
}

impl Capabilities {
    /// An empty skeleton.
    fn empty(source: CapabilitySource) -> Self {
        Self {
            chat: false,
            embedding: false,
            vision: false,
            tools: false,
            thinking: false,
            context_window: None,
            source,
        }
    }

    /// Guess from a descriptor string; ports `infer_capabilities`, branch order included - "embed" wins first, because `nomic-embed-vision` is an embedding model.
    pub fn infer(descriptor: &str) -> Self {
        let value = descriptor.to_lowercase();
        let mut caps = Self::empty(CapabilitySource::Inferred);
        if value.contains("embed") {
            caps.embedding = true;
            return caps;
        }
        caps.chat = true;
        caps.vision = VISION_TOKENS.iter().any(|token| value.contains(token));
        caps
    }

    /// Read the `capabilities` array from `/api/show`; `None` when nothing survives the filter, which tells the caller to fall through to guessing.
    pub fn from_reported(reported: &[String], context_window: Option<u64>) -> Option<Self> {
        let mut caps = Self::empty(CapabilitySource::Reported);
        let mut any = false;
        for raw in reported {
            let lowered = raw.to_lowercase();
            let name = OLLAMA_ALIASES
                .iter()
                .find(|(from, _)| *from == lowered)
                .map(|(_, to)| *to)
                .unwrap_or(lowered.as_str());
            if !OLLAMA_CAPABILITIES.contains(&name) {
                continue;
            }
            any = true;
            match name {
                "chat" => caps.chat = true,
                "embedding" => caps.embedding = true,
                "vision" => caps.vision = true,
                "tools" => caps.tools = true,
                "thinking" => caps.thinking = true,
                _ => {}
            }
        }
        if !any {
            return None;
        }
        caps.context_window = context_window;
        Some(caps)
    }

    /// Embedding only, no chat. The Python side classified `model_type` with exactly this comparison.
    pub fn is_embedding_only(&self) -> bool {
        self.embedding && !self.chat
    }

    /// The name list, for the UI and the database.
    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.chat {
            names.push("chat");
        }
        if self.embedding {
            names.push("embedding");
        }
        if self.vision {
            names.push("vision");
        }
        if self.tools {
            names.push("tools");
        }
        if self.thinking {
            names.push("thinking");
        }
        names
    }
}

/// Filter a raw `capabilities` array into normalized names, order kept and duplicates dropped.
pub fn normalize_ollama_capabilities(value: &Value) -> Vec<String> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let Some(raw) = item.as_str() else { continue };
        let lowered = raw.to_lowercase();
        let name = OLLAMA_ALIASES
            .iter()
            .find(|(from, _)| *from == lowered)
            .map(|(_, to)| (*to).to_string())
            .unwrap_or(lowered);
        if OLLAMA_CAPABILITIES.contains(&name.as_str()) && !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Find the context window in `/api/show`'s `model_info`; keys are architecture-prefixed, so suffix matching avoids maintaining an architecture table.
pub fn context_length_from_model_info(info: &Map<String, Value>) -> Option<u64> {
    info.iter()
        .find(|(key, _)| key.ends_with(".context_length") || *key == "context_length")
        .and_then(|(_, value)| value.as_u64())
}
