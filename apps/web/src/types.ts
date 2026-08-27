export type ServiceState = "online" | "offline" | "not_configured";

export interface Health {
  status: string;
  platform: string;
  services: Record<string, ServiceState>;
  gpu: {
    capacity_bytes: number;
    reserved_bytes: number;
    leases: Array<{
      owner: string;
      bytes_reserved: number;
      source: "reserved" | "observed";
    }>;
  };
}

export interface ModelInfo {
  name: string;
  model_type: string;
  state: "installed" | "loaded" | "unloaded" | "downloading" | "failed";
  size_bytes: number;
  vram_bytes: number;
  quantization?: string;
  capabilities: string[];
  runtime: string;
  sha256?: string;
  default_for: string[];
  error?: string;
}

export interface ModelEvent {
  id: string;
  model_name: string;
  action: string;
  status: "completed" | "failed";
  detail?: string;
  created_at: string;
}

export type ChatRole = "user" | "assistant" | "system";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

export interface ChatResponse {
  message?: ChatMessage;
  model?: string;
  done?: boolean;
}

export interface AsrResult {
  text: string;
  language: string;
  runtime: string;
}

export interface WorkspaceRecord {
  id: string;
  name: string;
  description: string;
  created_at: string;
  updated_at: string;
  conversation_count: number;
}

export interface ConversationRecord {
  id: string;
  workspace_id: string;
  title: string;
  model?: string;
  created_at: string;
  updated_at: string;
  message_count: number;
}

export interface PersistedMessage extends ChatMessage {
  id: string;
  conversation_id: string;
  created_at: string;
}

export interface ConversationDetail extends ConversationRecord {
  messages: PersistedMessage[];
}

export interface DocumentRecord {
  id: string;
  workspace_id: string;
  filename: string;
  media_type?: string;
  sha256: string;
  byte_size: number;
  status: "queued" | "processing" | "ready" | "needs_ocr" | "failed";
  extracted_text?: string;
  error?: string;
  created_at: string;
  updated_at: string;
}

export type MemoryType = "preference" | "fact" | "episodic";

export interface MemoryRecord {
  id: string;
  user_id: string;
  type: MemoryType;
  content: string;
  source: string;
  confidence: number;
  enabled: boolean;
  created_at: string;
  updated_at: string;
  expires_at?: string;
}
