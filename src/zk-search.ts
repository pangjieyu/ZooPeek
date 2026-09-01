import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { validateZkPath } from "./zk-path.ts";

// 对应 Rust 端 search.rs 的 SearchBuildState。snake_case 与 serde rename_all 对齐。
export type SearchBuildState =
  | "empty"
  | "building"
  | "ready"
  | "incomplete"
  | "truncated"
  | "failed"
  | "cancelled";

export interface SearchBuildStats {
  visited: number;
  inaccessible_subtrees: number;
  skipped_nodes: number;
  elapsed_ms: number;
  termination_reason: string | null;
}

export interface SearchIndexStatus {
  connection_epoch: number;
  generation: number;
  state: SearchBuildState;
  dirty: boolean;
  refreshing: boolean;
  built_at_ms: number | null;
  stats: SearchBuildStats;
}

export interface SearchIndexTicket {
  connection_epoch: number;
  generation: number;
  state: SearchBuildState;
}

export interface SearchIndexStateEvent {
  conn_id: string;
  connection_epoch: number;
  generation: number;
  state: SearchBuildState;
  dirty: boolean;
  stats: SearchBuildStats;
}

export interface SearchResult {
  path: string;
  name: string;
  score: number;
  match_target: "name" | "path";
  // Rust 侧返回 (usize, usize)，IPC 序列化后变成 number。
  highlight_ranges: [number, number][];
}

export interface SearchResponse {
  connection_epoch: number;
  index_generation: number;
  query_seq: number;
  snapshot_status: SearchBuildState;
  dirty: boolean;
  total_matches: number;
  results: SearchResult[];
}

/**
 * 前端 search API 封装：单次查询 + 一组序列号守卫。
 *
 * 关键约束：
 * - querySeq 由调用方提供（递增），本地拒绝晚到的旧响应，避免覆盖更新的输入。
 * - 后端的 connection_epoch / index_generation 由 Rust 端通过 status 命令确认；
 *   调用方传入当前 tab 持有的 epoch，Rust 拒绝不匹配（SEARCH_CONNECTION_CHANGED）。
 * - 不直接缓存任何远端状态：本地 querySeq + 远端 epoch 双向拦截足够防止错乱覆盖。
 */

export async function startSearchIndex(
  connId: string,
  force = false
): Promise<SearchIndexTicket> {
  return invoke<SearchIndexTicket>("start_search_index", {
    connId,
    force,
  });
}

export async function getSearchIndexStatus(
  connId: string
): Promise<SearchIndexStatus> {
  return invoke<SearchIndexStatus>("get_search_index_status", { connId });
}

export async function cancelSearchIndex(
  connId: string,
  connectionEpoch: number,
  generation: number
): Promise<void> {
  await invoke("cancel_search_index", {
    connId,
    connectionEpoch,
    generation,
  });
}

export async function searchNodes(request: {
  connId: string;
  connectionEpoch: number;
  querySeq: number;
  query: string;
  scopePath: string;
  limit: number;
}): Promise<SearchResponse> {
  return invoke<SearchResponse>("search_nodes", { request });
}

export async function onSearchIndexState(
  handler: (event: SearchIndexStateEvent) => void
): Promise<UnlistenFn> {
  return listen<SearchIndexStateEvent>("zk-search-index-state", (event) => {
    handler(event.payload);
  });
}

/**
 * 校验搜索关键字是否合法。空查询与首尾 `/` 视为非法，与 Rust 端 validate_zk_path 同步。
 */
export function isValidSearchQuery(query: string): boolean {
  const trimmed = query.trim();
  if (trimmed.length === 0) return false;
  if (trimmed.includes("/") && !validateZkPath(trimmed)) return false;
  return true;
}

/**
 * 比 highlight_ranges 更高的优先级：完全相等 > 前缀 > 包含 > 子序列。
 * 该排序供 UI 高亮层级使用，保留 Rust 端的稳定顺序以保证行为一致。
 */
export function describeMatchHighlight(result: SearchResult): string {
  if (result.match_target === "path") return "路径匹配";
  if (result.highlight_ranges.length === 0) return "子序列匹配";
  const [start, end] = result.highlight_ranges[0];
  if (start === 0 && end === result.name.length) return "完全匹配";
  if (start === 0) return "前缀匹配";
  return "包含匹配";
}