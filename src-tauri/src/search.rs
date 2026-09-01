use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{async_runtime::JoinHandle, AppHandle, Emitter};
use tokio::sync::Semaphore;
use zookeeper_client as zk;

const MAX_RESULTS: usize = 500;
const MAX_NODES: usize = 200_000;
const BUILD_TIMEOUT: Duration = Duration::from_secs(120);
const PER_CONNECTION_CONCURRENCY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchBuildState {
    Empty,
    Building,
    Ready,
    Incomplete,
    Truncated,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SearchBuildStats {
    pub visited: usize,
    pub inaccessible_subtrees: usize,
    pub skipped_nodes: usize,
    pub elapsed_ms: u64,
    pub termination_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchIndexStatus {
    pub connection_epoch: u64,
    pub generation: u64,
    pub state: SearchBuildState,
    pub dirty: bool,
    pub refreshing: bool,
    pub built_at_ms: Option<u64>,
    pub stats: SearchBuildStats,
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchIndexTicket {
    pub connection_epoch: u64,
    pub generation: u64,
    pub state: SearchBuildState,
}

#[derive(Clone, Debug)]
pub struct SearchSnapshot {
    pub connection_epoch: u64,
    pub generation: u64,
    pub paths: Arc<Vec<String>>,
    pub state: SearchBuildState,
    pub stats: SearchBuildStats,
    pub built_at_ms: u64,
}

impl SearchSnapshot {
        pub fn for_test(connection_epoch: u64, generation: u64, paths: Vec<String>) -> Self {
        Self {
            connection_epoch,
            generation,
            paths: Arc::new(paths),
            state: SearchBuildState::Ready,
            stats: SearchBuildStats::default(),
            built_at_ms: 0,
        }
    }
}

struct SearchSlot {
    connection_epoch: u64,
    generation: u64,
    state: SearchBuildState,
    task: Option<JoinHandle<()>>,
    snapshot: Option<Arc<SearchSnapshot>>,
    dirty: bool,
    stats: SearchBuildStats,
}

#[derive(Clone)]
pub struct SearchManager {
    slots: Arc<Mutex<HashMap<String, SearchSlot>>>,
    next_epoch: Arc<AtomicU64>,
    pub global_budget: Arc<Semaphore>,
}

impl Default for SearchManager {
    fn default() -> Self { Self::new() }
}

impl SearchManager {
    pub fn new() -> Self {
        Self {
            slots: Arc::new(Mutex::new(HashMap::new())),
            next_epoch: Arc::new(AtomicU64::new(0)),
            global_budget: Arc::new(Semaphore::new(32)),
        }
    }

    pub fn install_connection(&self, conn_id: &str) -> Result<u64, String> {
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed) + 1;
        let old_task = self
            .slots
            .lock()
            .map_err(|error| error.to_string())?
            .insert(
                conn_id.to_string(),
                SearchSlot {
                    connection_epoch: epoch,
                    generation: 0,
                    state: SearchBuildState::Empty,
                    task: None,
                    snapshot: None,
                    dirty: false,
                    stats: SearchBuildStats::default(),
                },
            )
            .and_then(|mut slot| slot.task.take());
        if let Some(task) = old_task {
            task.abort();
        }
        Ok(epoch)
    }

    pub fn remove_connection(&self, conn_id: &str) -> Result<(), String> {
        let old_task = self
            .slots
            .lock()
            .map_err(|error| error.to_string())?
            .remove(conn_id)
            .and_then(|mut slot| slot.task.take());
        if let Some(task) = old_task {
            task.abort();
        }
        Ok(())
    }

    pub fn begin_build(&self, conn_id: &str, force: bool) -> Result<SearchIndexTicket, String> {
        let mut slots = self.slots.lock().map_err(|error| error.to_string())?;
        let slot = slots
            .get_mut(conn_id)
            .ok_or_else(|| format!("连接 {conn_id} 不存在或已断开"))?;
        if !force && (slot.state == SearchBuildState::Building || slot.snapshot.is_some()) {
            return Ok(SearchIndexTicket {
                connection_epoch: slot.connection_epoch,
                generation: slot.generation,
                state: slot.state,
            });
        }
        slot.generation += 1;
        slot.state = SearchBuildState::Building;
        slot.stats = SearchBuildStats::default();
        Ok(SearchIndexTicket {
            connection_epoch: slot.connection_epoch,
            generation: slot.generation,
            state: slot.state,
        })
    }

    pub fn replace_task(
        &self,
        conn_id: &str,
        epoch: u64,
        generation: u64,
        task: JoinHandle<()>,
    ) -> Result<(), String> {
        let old_task = {
            let mut slots = self.slots.lock().map_err(|error| error.to_string())?;
            let Some(slot) = slots.get_mut(conn_id) else {
                task.abort();
                return Ok(());
            };
            if slot.connection_epoch != epoch || slot.generation != generation {
                task.abort();
                return Ok(());
            }
            slot.task.replace(task)
        };
        if let Some(old_task) = old_task {
            old_task.abort();
        }
        Ok(())
    }

    pub fn publish(
        &self,
        conn_id: &str,
        epoch: u64,
        generation: u64,
        snapshot: Arc<SearchSnapshot>,
    ) -> bool {
        let Ok(mut slots) = self.slots.lock() else {
            return false;
        };
        let Some(slot) = slots.get_mut(conn_id) else {
            return false;
        };
        if slot.connection_epoch != epoch || slot.generation != generation {
            return false;
        }
        slot.state = snapshot.state;
        slot.stats = snapshot.stats.clone();
        slot.snapshot = Some(snapshot);
        slot.dirty = false;
        slot.task = None;
        true
    }

    pub fn fail_build(&self, conn_id: &str, epoch: u64, generation: u64, reason: String) -> bool {
        let Ok(mut slots) = self.slots.lock() else {
            return false;
        };
        let Some(slot) = slots.get_mut(conn_id) else {
            return false;
        };
        if slot.connection_epoch != epoch || slot.generation != generation {
            return false;
        }
        slot.state = SearchBuildState::Failed;
        slot.stats.termination_reason = Some(reason);
        slot.task = None;
        true
    }

    pub fn update_progress(
        &self,
        conn_id: &str,
        epoch: u64,
        generation: u64,
        stats: &SearchBuildStats,
    ) -> bool {
        let Ok(mut slots) = self.slots.lock() else {
            return false;
        };
        let Some(slot) = slots.get_mut(conn_id) else {
            return false;
        };
        if slot.connection_epoch != epoch || slot.generation != generation {
            return false;
        }
        slot.stats = stats.clone();
        true
    }

    pub fn cancel_build(&self, conn_id: &str, epoch: u64, generation: u64) -> Result<(), String> {
        let task = {
            let mut slots = self.slots.lock().map_err(|error| error.to_string())?;
            let Some(slot) = slots.get_mut(conn_id) else {
                return Ok(());
            };
            if slot.connection_epoch != epoch || slot.generation != generation {
                return Ok(());
            }
            slot.state = if let Some(snapshot) = &slot.snapshot {
                snapshot.state
            } else {
                SearchBuildState::Cancelled
            };
            slot.task.take()
        };
        if let Some(task) = task {
            task.abort();
        }
        Ok(())
    }

    pub fn mark_dirty(&self, conn_id: &str) -> Result<(), String> {
        if let Some(slot) = self
            .slots
            .lock()
            .map_err(|error| error.to_string())?
            .get_mut(conn_id)
        {
            if slot.snapshot.is_some() {
                slot.dirty = true;
            }
        }
        Ok(())
    }

    pub fn snapshot(&self, conn_id: &str) -> Result<Option<Arc<SearchSnapshot>>, String> {
        Ok(self
            .slots
            .lock()
            .map_err(|error| error.to_string())?
            .get(conn_id)
            .and_then(|slot| slot.snapshot.clone()))
    }

    pub fn status(&self, conn_id: &str) -> Result<SearchIndexStatus, String> {
        let slots = self.slots.lock().map_err(|error| error.to_string())?;
        let slot = slots
            .get(conn_id)
            .ok_or_else(|| format!("连接 {conn_id} 不存在或已断开"))?;
        Ok(SearchIndexStatus {
            connection_epoch: slot.connection_epoch,
            generation: slot.generation,
            state: slot.state,
            dirty: slot.dirty,
            refreshing: slot.state == SearchBuildState::Building && slot.snapshot.is_some(),
            built_at_ms: slot.snapshot.as_ref().map(|snapshot| snapshot.built_at_ms),
            stats: slot.stats.clone(),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SearchIndexStateEvent {
    pub conn_id: String,
    pub connection_epoch: u64,
    pub generation: u64,
    pub state: SearchBuildState,
    pub dirty: bool,
    pub stats: SearchBuildStats,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub conn_id: String,
    pub connection_epoch: u64,
    pub query_seq: u64,
    pub query: String,
    pub scope_path: String,
    pub limit: usize,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub connection_epoch: u64,
    pub index_generation: u64,
    pub query_seq: u64,
    pub snapshot_status: SearchBuildState,
    pub dirty: bool,
    pub total_matches: usize,
    pub results: Vec<SearchResult>,
}

fn emit_state(app: &AppHandle, conn_id: &str, status: &SearchIndexStatus) {
    let _ = app.emit(
        "zk-search-index-state",
        SearchIndexStateEvent {
            conn_id: conn_id.to_string(),
            connection_epoch: status.connection_epoch,
            generation: status.generation,
            state: status.state,
            dirty: status.dirty,
            stats: status.stats.clone(),
        },
    );
}

fn child_path(parent: &str, child: &str) -> String {
    if parent == "/" {
        format!("/{child}")
    } else {
        format!("{parent}/{child}")
    }
}

enum ListChildrenError {
    NoNode,
    NoAuth,
    Deadline,
    Fatal(String),
}

async fn list_children_for_search(
    client: zk::Client,
    path: &str,
    budget: Arc<Semaphore>,
    deadline: Instant,
) -> Result<Vec<String>, ListChildrenError> {
    for attempt in 0..=2 {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(ListChildrenError::Deadline)?;
        let permit = tokio::time::timeout(remaining, budget.clone().acquire_owned())
            .await
            .map_err(|_| ListChildrenError::Deadline)?
            .map_err(|error| ListChildrenError::Fatal(error.to_string()))?;
        let result = tokio::time::timeout(remaining, client.list_children(path)).await;
        drop(permit);
        match result {
            Err(_) => return Err(ListChildrenError::Deadline),
            Ok(Ok(mut children)) => {
                children.sort();
                return Ok(children);
            }
            Ok(Err(zk::Error::NoNode)) => return Err(ListChildrenError::NoNode),
            Ok(Err(zk::Error::NoAuth)) => return Err(ListChildrenError::NoAuth),
            Ok(Err(zk::Error::ConnectionLoss)) if attempt < 2 => {
                tokio::time::sleep(if attempt == 0 {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(300)
                })
                .await;
            }
            Ok(Err(error)) => return Err(ListChildrenError::Fatal(error.to_string())),
        }
    }
    Err(ListChildrenError::Fatal("读取节点列表失败".to_string()))
}

async fn build_snapshot(
    client: zk::Client,
    manager: SearchManager,
    conn_id: String,
    epoch: u64,
    generation: u64,
) -> Result<SearchSnapshot, String> {
    let started = Instant::now();
    let deadline = started + BUILD_TIMEOUT;
    let mut paths = vec!["/".to_string()];
    let mut current_level = vec!["/".to_string()];
    let mut stats = SearchBuildStats {
        visited: 1,
        ..SearchBuildStats::default()
    };
    let mut state = SearchBuildState::Ready;

    while !current_level.is_empty() {
        if Instant::now() >= deadline {
            state = SearchBuildState::Truncated;
            stats.termination_reason = Some("deadline".to_string());
            break;
        }
        current_level.sort();
        current_level.dedup();
        let results = stream::iter(current_level.into_iter().enumerate())
            .map(|(index, parent)| {
                let client = client.clone();
                let budget = manager.global_budget.clone();
                async move {
                    let result = list_children_for_search(client, &parent, budget, deadline).await;
                    (index, parent, result)
                }
            })
            .buffer_unordered(PER_CONNECTION_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        let mut results = results;
        results.sort_by_key(|(index, _, _)| *index);
        let mut next_level = Vec::new();
        for (_, parent, result) in results {
            match result {
                Ok(children) => {
                    for child in children {
                        if paths.len() >= MAX_NODES {
                            state = SearchBuildState::Truncated;
                            stats.termination_reason = Some("node_limit".to_string());
                            break;
                        }
                        let path = child_path(&parent, &child);
                        paths.push(path.clone());
                        next_level.push(path);
                    }
                }
                Err(ListChildrenError::NoNode) => stats.skipped_nodes += 1,
                Err(ListChildrenError::NoAuth) => {
                    stats.inaccessible_subtrees += 1;
                    if state != SearchBuildState::Truncated {
                        state = SearchBuildState::Incomplete;
                    }
                }
                Err(ListChildrenError::Deadline) => {
                    state = SearchBuildState::Truncated;
                    stats.termination_reason = Some("deadline".to_string());
                }
                Err(ListChildrenError::Fatal(error)) => return Err(error),
            }
            if state == SearchBuildState::Truncated {
                break;
            }
        }
        stats.visited = paths.len();
        stats.elapsed_ms = started.elapsed().as_millis() as u64;
        if !manager.update_progress(&conn_id, epoch, generation, &stats) {
            return Err("SEARCH_CANCELLED".to_string());
        }
        if state == SearchBuildState::Truncated {
            break;
        }
        current_level = next_level;
    }

    stats.visited = paths.len();
    stats.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(SearchSnapshot {
        connection_epoch: epoch,
        generation,
        paths: Arc::new(paths),
        state,
        stats,
        built_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
    })
}

pub fn start_index_build(
    app: AppHandle,
    manager: SearchManager,
    client: zk::Client,
    conn_id: String,
    force: bool,
) -> Result<SearchIndexTicket, String> {
    let ticket = manager.begin_build(&conn_id, force)?;
    if ticket.state != SearchBuildState::Building {
        return Ok(ticket);
    }
    let epoch = ticket.connection_epoch;
    let generation = ticket.generation;
    let task_manager = manager.clone();
    let task_conn_id = conn_id.clone();
    let task_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        match build_snapshot(
            client,
            task_manager.clone(),
            task_conn_id.clone(),
            epoch,
            generation,
        )
        .await
        {
            Ok(snapshot) => {
                task_manager.publish(&task_conn_id, epoch, generation, Arc::new(snapshot));
            }
            Err(error) if error == "SEARCH_CANCELLED" => return,
            Err(error) => {
                task_manager.fail_build(&task_conn_id, epoch, generation, error);
            }
        }
        if let Ok(status) = task_manager.status(&task_conn_id) {
            emit_state(&task_app, &task_conn_id, &status);
        }
    });
    manager.replace_task(&conn_id, epoch, generation, task)?;
    if let Ok(status) = manager.status(&conn_id) {
        emit_state(&app, &conn_id, &status);
    }
    Ok(ticket)
}

pub async fn search_snapshot(
    manager: SearchManager,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    let snapshot = manager
        .snapshot(&request.conn_id)?
        .ok_or_else(|| "INDEX_NOT_READY".to_string())?;
    if snapshot.connection_epoch != request.connection_epoch {
        return Err("SEARCH_CONNECTION_CHANGED".to_string());
    }
    let status = manager.status(&request.conn_id)?;
    let paths = snapshot.paths.clone();
    let query = request.query.clone();
    let scope_path = request.scope_path.clone();
    let limit = request.limit.min(MAX_RESULTS);
    let matches = tauri::async_runtime::spawn_blocking(move || {
        search_paths_with_count(&paths, &query, &scope_path, limit)
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(SearchResponse {
        connection_epoch: snapshot.connection_epoch,
        index_generation: snapshot.generation,
        query_seq: request.query_seq,
        snapshot_status: snapshot.state,
        dirty: status.dirty,
        total_matches: matches.total_matches,
        results: matches.results,
    })
}

pub fn validate_zk_path(path: &str) -> bool {
    if path == "/" {
        return true;
    }
    if !path.starts_with('/') || path.ends_with('/') {
        return false;
    }
    path[1..]
        .split('/')
        .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

pub fn is_path_in_scope(path: &str, scope_path: &str) -> bool {
    if !validate_zk_path(path) || !validate_zk_path(scope_path) {
        return false;
    }
    scope_path == "/"
        || path == scope_path
        || path
            .strip_prefix(scope_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn subsequence_score(target: &str, query: &str) -> Option<(i64, Vec<(usize, usize)>)> {
    let target_chars = target.chars().collect::<Vec<_>>();
    let query_chars = query.chars().collect::<Vec<_>>();
    let mut positions = Vec::with_capacity(query_chars.len());
    let mut cursor = 0;
    for query_char in query_chars {
        let relative = target_chars[cursor..]
            .iter()
            .position(|target_char| *target_char == query_char)?;
        cursor += relative;
        positions.push(cursor);
        cursor += 1;
    }
    let gaps = positions
        .windows(2)
        .map(|window| window[1].saturating_sub(window[0] + 1))
        .sum::<usize>();
    let ranges = positions
        .into_iter()
        .map(|index| (index, index + 1))
        .collect();
    Some((
        100_000 - gaps as i64 * 100 - target_chars.len() as i64,
        ranges,
    ))
}

fn match_score(target: &str, query: &str) -> Option<(i64, Vec<(usize, usize)>)> {
    if target == query {
        return Some((
            400_000 - target.chars().count() as i64,
            vec![(0, query.chars().count())],
        ));
    }
    if target.starts_with(query) {
        return Some((
            300_000 - target.chars().count() as i64,
            vec![(0, query.chars().count())],
        ));
    }
    if let Some(byte_offset) = target.find(query) {
        let start = target[..byte_offset].chars().count();
        return Some((
            200_000 - start as i64 * 100 - target.chars().count() as i64,
            vec![(start, start + query.chars().count())],
        ));
    }
    subsequence_score(target, query)
}

struct SearchMatches {
    total_matches: usize,
    results: Vec<SearchResult>,
}

fn search_paths_with_count(
    paths: &[String],
    query: &str,
    scope_path: &str,
    limit: usize,
) -> Result<SearchMatches, String> {
    if !validate_zk_path(scope_path) {
        return Err("搜索范围不是合法的 ZooKeeper 路径".to_string());
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() || limit == 0 {
        return Ok(SearchMatches {
            total_matches: 0,
            results: Vec::new(),
        });
    }
    if query.contains('/') && !validate_zk_path(&query) {
        return Err("路径查询不是合法的 ZooKeeper 路径".to_string());
    }
    let match_path = query.contains('/');
    let mut results = paths
        .iter()
        .filter(|path| is_path_in_scope(path, scope_path))
        .filter_map(|path| {
            let name = path.rsplit('/').next().unwrap_or("/");
            let target = if match_path { path.as_str() } else { name };
            let folded = target.to_lowercase();
            let (score, highlight_ranges) = match_score(&folded, &query)?;
            Some(SearchResult {
                path: path.clone(),
                name: if path == "/" {
                    "/".to_string()
                } else {
                    name.to_string()
                },
                score,
                match_target: if match_path { "path" } else { "name" },
                highlight_ranges,
            })
        })
        .collect::<Vec<_>>();
    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| {
                left.path
                    .matches('/')
                    .count()
                    .cmp(&right.path.matches('/').count())
            })
            .then_with(|| left.path.cmp(&right.path))
    });
    let total_matches = results.len();
    results.truncate(limit.min(MAX_RESULTS));
    Ok(SearchMatches {
        total_matches,
        results,
    })
}

pub fn search_paths(
    paths: &[String],
    query: &str,
    scope_path: &str,
    limit: usize,
) -> Result<Vec<SearchResult>, String> {
    search_paths_with_count(paths, query, scope_path, limit).map(|matches| matches.results)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub score: i64,
    pub match_target: &'static str,
    pub highlight_ranges: Vec<(usize, usize)>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        is_path_in_scope, search_paths, validate_zk_path, SearchBuildState, SearchManager,
        SearchSnapshot,
    };

    #[test]
    fn validates_canonical_zookeeper_paths() {
        for path in ["/", "/a", "/中文/节点"] {
            assert!(validate_zk_path(path), "{path}");
        }
        for path in ["", "a", "/a/", "/a//b", "/./a", "/a/../b"] {
            assert!(!validate_zk_path(path), "{path}");
        }
    }

    #[test]
    fn scope_uses_path_segment_boundaries() {
        assert!(is_path_in_scope("/foo", "/foo"));
        assert!(is_path_in_scope("/foo/bar", "/foo"));
        assert!(!is_path_in_scope("/foobar", "/foo"));
        assert!(is_path_in_scope("/anything", "/"));
    }

    #[test]
    fn basename_and_path_queries_have_stable_top_k_order() {
        let paths = vec![
            "/services/order-api".to_string(),
            "/archive/order-api".to_string(),
            "/services/orders".to_string(),
            "/services/payment".to_string(),
        ];
        let by_name = search_paths(&paths, "order", "/", 10).unwrap();
        assert_eq!(
            by_name
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/services/orders",
                "/archive/order-api",
                "/services/order-api"
            ]
        );
        assert!(by_name.iter().all(|item| item.match_target == "name"));

        let by_path = search_paths(&paths, "/services/order", "/", 2).unwrap();
        assert_eq!(
            by_path
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/services/orders", "/services/order-api"]
        );
        assert!(by_path.iter().all(|item| item.match_target == "path"));
    }

    #[test]
    fn search_counts_all_matches_before_limiting_results() {
        let paths = vec![
            "/a/item".to_string(),
            "/b/item".to_string(),
            "/c/item".to_string(),
        ];
        let result = search_paths(&paths, "item", "/", 2).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn stale_epoch_or_generation_cannot_publish_a_snapshot() {
        let manager = SearchManager::new();
        let epoch = manager.install_connection("conn").unwrap();
        let ticket = manager.begin_build("conn", false).unwrap();
        let snapshot = Arc::new(SearchSnapshot::for_test(
            epoch,
            ticket.generation,
            vec!["/".to_string()],
        ));
        assert!(manager.publish("conn", epoch, ticket.generation, snapshot.clone()));

        let new_epoch = manager.install_connection("conn").unwrap();
        assert_ne!(epoch, new_epoch);
        assert!(!manager.publish("conn", epoch, ticket.generation, snapshot));
        assert!(manager.snapshot("conn").unwrap().is_none());
    }

    #[test]
    fn known_structure_changes_only_mark_existing_snapshot_dirty() {
        let manager = SearchManager::new();
        let epoch = manager.install_connection("conn").unwrap();
        let ticket = manager.begin_build("conn", false).unwrap();
        let snapshot = Arc::new(SearchSnapshot::for_test(
            epoch,
            ticket.generation,
            vec!["/".to_string()],
        ));
        assert!(manager.publish("conn", epoch, ticket.generation, snapshot));

        manager.mark_dirty("conn").unwrap();
        let status = manager.status("conn").unwrap();
        assert_eq!(status.state, SearchBuildState::Ready);
        assert!(status.dirty);
    }
}
