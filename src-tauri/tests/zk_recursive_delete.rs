//! 递归删除真实链路测试，直接调用生产实现。
//! 依赖本地 docker ZK（127.0.0.1:2181）。

use zookeeper_client as zk;
use zoopeek_lib::delete_node_tree;

const CLUSTER: &str = "127.0.0.1:2181";
const DEEP_ROOT: &str = "/zoopeek-delete-deep";
const RACE_ROOT: &str = "/zoopeek-delete-race";

async fn create(client: &zk::Client, path: &str) {
    let options = zk::CreateMode::Persistent.with_acls(zk::Acls::anyone_all());
    client
        .create(path, b"x", &options)
        .await
        .expect("create failed");
}

#[tokio::test(flavor = "multi_thread")]
async fn recursive_delete_deep_tree() {
    let client = zk::Client::connect(CLUSTER).await.expect("connect failed");
    let _ = delete_node_tree(&client, DEEP_ROOT).await;

    for path in [
        DEEP_ROOT,
        "/zoopeek-delete-deep/a",
        "/zoopeek-delete-deep/a/b",
        "/zoopeek-delete-deep/a/b/c",
        "/zoopeek-delete-deep/a/b/d",
        "/zoopeek-delete-deep/e",
    ] {
        create(&client, path).await;
    }

    let deleted = delete_node_tree(&client, DEEP_ROOT)
        .await
        .expect("recursive delete failed");
    assert_eq!(deleted, 6);
    assert_eq!(client.get_data(DEEP_ROOT).await, Err(zk::Error::NoNode));
    println!("RECURSIVE DELETE TEST PASSED: deleted {deleted} nodes");
}

#[tokio::test(flavor = "multi_thread")]
async fn recursive_delete_retries_when_child_is_created_concurrently() {
    let observer = zk::Client::connect(CLUSTER)
        .await
        .expect("observer connect failed");
    let deleter = zk::Client::connect(CLUSTER)
        .await
        .expect("deleter connect failed");
    let writer = zk::Client::connect(CLUSTER)
        .await
        .expect("writer connect failed");
    let _ = delete_node_tree(&observer, RACE_ROOT).await;

    create(&observer, RACE_ROOT).await;
    for index in 0..64 {
        create(&observer, &format!("{RACE_ROOT}/child-{index:02}")).await;
    }

    let (_, _, watcher) = observer
        .get_and_watch_children(RACE_ROOT)
        .await
        .expect("watch race root failed");
    let task = tokio::spawn(async move { delete_node_tree(&deleter, RACE_ROOT).await });

    let event = watcher.changed().await;
    assert_eq!(event.event_type, zk::EventType::NodeChildrenChanged);
    create(&writer, &format!("{RACE_ROOT}/late-child")).await;

    let deleted = task
        .await
        .expect("delete task panicked")
        .expect("recursive delete should absorb concurrent child creation");
    assert_eq!(deleted, 66);
    assert_eq!(observer.get_data(RACE_ROOT).await, Err(zk::Error::NoNode));
    println!("CONCURRENT CREATE RETRY TEST PASSED: deleted {deleted} nodes");
}
