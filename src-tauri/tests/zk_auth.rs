use zookeeper_client::{Client, Error, SaslOptions};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("缺少环境变量 {name}"))
}

#[tokio::test]
#[ignore = "需要 127.0.0.1:2181 上启用 digest 和 ACL 的 ZooKeeper"]
async fn digest_auth_and_wrong_password_no_auth() {
    let servers =
        std::env::var("ZOOPEEK_ZK_SERVERS").unwrap_or_else(|_| "127.0.0.1:2181".to_string());
    let username = env("ZOOPEEK_ZK_DIGEST_USER");
    let password = env("ZOOPEEK_ZK_DIGEST_PASSWORD");
    let protected_path = env("ZOOPEEK_ZK_PROTECTED_PATH");

    let auth = format!("{username}:{password}");
    let client = Client::connector()
        .with_auth("digest", auth.as_bytes())
        .connect(&servers)
        .await
        .expect("digest 连接应成功");
    client
        .get_data(&protected_path)
        .await
        .expect("正确 digest 凭证应能读取受限节点");

    let wrong_auth = format!("{username}:wrong-password");
    let wrong_client = Client::connector()
        .with_auth("digest", wrong_auth.as_bytes())
        .connect(&servers)
        .await
        .expect("digest 错密码通常仍能建立连接");
    assert_eq!(
        wrong_client.get_data(&protected_path).await.unwrap_err(),
        Error::NoAuth
    );
}

#[tokio::test]
#[ignore = "需要 127.0.0.1:2181 上启用 SASL DIGEST-MD5 的 ZooKeeper"]
async fn sasl_digest_md5_auth() {
    let servers =
        std::env::var("ZOOPEEK_ZK_SERVERS").unwrap_or_else(|_| "127.0.0.1:2181".to_string());
    let username = env("ZOOPEEK_ZK_SASL_USER");
    let password = env("ZOOPEEK_ZK_SASL_PASSWORD");
    let protected_path = env("ZOOPEEK_ZK_PROTECTED_PATH");

    let client = Client::connector()
        .with_sasl(SaslOptions::digest_md5(username, password))
        .connect(&servers)
        .await
        .expect("SASL DIGEST-MD5 连接应成功");
    client
        .get_data(&protected_path)
        .await
        .expect("正确 SASL 凭证应能读取受限节点");
}
