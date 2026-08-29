//! ACL 真实往返测试：验证父 ACL 继承、get_acl / set_acl 与权限位组合。
//! 依赖本地 docker ZK：127.0.0.1:2181。

use zookeeper_client as zk;

const CLUSTER: &str = "127.0.0.1:2181";
const PARENT: &str = "/zoopeek-acl-test";
const CHILD: &str = "/zoopeek-acl-test/child";

#[tokio::test(flavor = "multi_thread")]
async fn acl_round_trip_and_inheritance() {
    let client = zk::Client::connect(CLUSTER).await.expect("connect failed");
    let _ = client.delete(CHILD, None).await;
    let _ = client.delete(PARENT, None).await;

    let create_options = zk::CreateMode::Persistent.with_acls(zk::Acls::anyone_all());
    client
        .create(PARENT, b"acl", &create_options)
        .await
        .expect("create ACL test parent failed");

    // 与 create_node 命令保持一致：显式读取并复制父节点 ACL。
    let (parent_acl, _) = client.get_acl(PARENT).await.expect("get parent ACL failed");
    let child_options = zk::CreateMode::Persistent.with_acls(zk::Acls::new(&parent_acl));
    client
        .create(CHILD, b"child", &child_options)
        .await
        .expect("create child with inherited ACL failed");
    let (child_acl, _) = client.get_acl(CHILD).await.expect("get child ACL failed");
    assert_eq!(child_acl, parent_acl);

    let read_admin = zk::Permission::READ | zk::Permission::ADMIN;
    let restricted_acl = [zk::Acl::new(read_admin, zk::AuthId::anyone())];
    client
        .set_acl(CHILD, &restricted_acl, None)
        .await
        .expect("set restricted ACL failed");

    let (actual, _) = client
        .get_acl(CHILD)
        .await
        .expect("get restricted ACL failed");
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].scheme(), "world");
    assert_eq!(actual[0].id(), "anyone");
    assert!(actual[0].permission().has(zk::Permission::READ));
    assert!(actual[0].permission().has(zk::Permission::ADMIN));
    assert!(!actual[0].permission().has(zk::Permission::WRITE));

    let full_acl = [zk::Acl::new(zk::Permission::ALL, zk::AuthId::anyone())];
    client
        .set_acl(CHILD, &full_acl, None)
        .await
        .expect("restore full ACL failed");
    let (actual, _) = client.get_acl(CHILD).await.expect("get full ACL failed");
    assert_eq!(actual[0].permission(), zk::Permission::ALL);

    client
        .delete(CHILD, None)
        .await
        .expect("cleanup ACL child failed");
    client
        .delete(PARENT, None)
        .await
        .expect("cleanup ACL parent failed");
    println!("ACL INHERITANCE AND ROUND TRIP TEST PASSED");
}
