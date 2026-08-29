use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, State};
use zookeeper_client as zk;

struct AppState {
    client: Mutex<Option<zk::Client>>,
}

#[derive(Serialize)]
struct ConnectResult {
    session_id: String,
}

#[derive(Serialize, Clone)]
struct NodeEvent {
    event_type: String,
    path: String,
    zxid: i64,
}

fn get_client(state: &State<'_, AppState>) -> Result<zk::Client, String> {
    state
        .client
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "not connected".to_string())
}

#[tauri::command]
async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    servers: String,
) -> Result<ConnectResult, String> {
    let client = zk::Client::connect(&servers)
        .await
        .map_err(|e| e.to_string())?;
    let session_id = client.session_id().to_string();

    // 会话状态变化实时推给前端（CONNECTING/DISCONNECTED/SYNC_CONNECTED/EXPIRED...）
    let mut state_watcher = client.state_watcher();
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 先推当前状态（连接成功时通常已是 SyncConnected），否则前端会停在 CONNECTING
        let _ = app_handle.emit("zk-session-state", format!("{:?}", state_watcher.state()));
        loop {
            let session_state = state_watcher.changed().await;
            let _ = app_handle.emit("zk-session-state", format!("{:?}", session_state));
            if matches!(
                session_state,
                zk::SessionState::Expired | zk::SessionState::Closed | zk::SessionState::AuthFailed
            ) {
                break;
            }
        }
    });

    *state.client.lock().map_err(|e| e.to_string())? = Some(client);
    Ok(ConnectResult { session_id })
}

#[tauri::command]
async fn list_children(state: State<'_, AppState>, path: String) -> Result<Vec<String>, String> {
    let client = get_client(&state)?;
    let mut children = client
        .list_children(&path)
        .await
        .map_err(|e| e.to_string())?;
    children.sort();
    Ok(children)
}

/// 读取子节点列表并挂一次性 watcher，触发后以 zk-node-event 推给前端。
/// ZK watcher 是一次性的，前端收到事件后需重新调用本命令刷新并重新挂 watch。
#[tauri::command]
async fn watch_children(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<String>, String> {
    let client = get_client(&state)?;
    let (mut children, _stat, watcher) = client
        .get_and_watch_children(&path)
        .await
        .map_err(|e| e.to_string())?;
    children.sort();
    tauri::async_runtime::spawn(async move {
        let event = watcher.changed().await;
        let _ = app.emit(
            "zk-node-event",
            NodeEvent {
                event_type: format!("{:?}", event.event_type),
                path: event.path,
                zxid: event.zxid,
            },
        );
    });
    Ok(children)
}

#[tauri::command]
async fn get_data(state: State<'_, AppState>, path: String) -> Result<String, String> {
    let client = get_client(&state)?;
    let (data, _stat) = client.get_data(&path).await.map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&data).to_string())
}

/// 同 watch_children，但watch 节点数据变化。
#[tauri::command]
async fn watch_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let client = get_client(&state)?;
    let (data, _stat, watcher) = client
        .get_and_watch_data(&path)
        .await
        .map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn(async move {
        let event = watcher.changed().await;
        let _ = app.emit(
            "zk-node-event",
            NodeEvent {
                event_type: format!("{:?}", event.event_type),
                path: event.path,
                zxid: event.zxid,
            },
        );
    });
    Ok(String::from_utf8_lossy(&data).to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            client: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            connect,
            list_children,
            watch_children,
            get_data,
            watch_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
