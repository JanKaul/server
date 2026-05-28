//! Stage 0 checkpoint 3 smoke test: SlateDB runs in our toolchain against Minio.
//!
//! Self-contained: testcontainers spawns Minio, aws-sdk-s3 creates the bucket,
//! SlateDB does one put/get roundtrip. No external setup required beyond a
//! working podman/docker socket.

use std::sync::Arc;

use aws_config::BehaviorVersion;
use aws_sdk_s3::config::Credentials;
use slatedb::object_store::aws::AmazonS3Builder;
use slatedb::Db;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::minio::MinIO;

const BUCKET: &str = "slatedb-test";
const ACCESS_KEY: &str = "minioadmin";
const SECRET_KEY: &str = "minioadmin";
const REGION: &str = "us-east-1";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_get_roundtrip_against_minio() {
    let container = MinIO::default()
        .start()
        .await
        .expect("start minio container");
    let port = container
        .get_host_port_ipv4(9000)
        .await
        .expect("minio api port");
    let endpoint = format!("http://127.0.0.1:{port}");

    create_bucket(&endpoint, BUCKET).await;

    let store = Arc::new(
        AmazonS3Builder::new()
            .with_endpoint(&endpoint)
            .with_bucket_name(BUCKET)
            .with_access_key_id(ACCESS_KEY)
            .with_secret_access_key(SECRET_KEY)
            .with_region(REGION)
            .with_allow_http(true)
            .build()
            .expect("build object_store s3"),
    );

    let db = Db::open("/checkpoint3", store)
        .await
        .expect("slatedb open");
    db.put(b"hello", b"world").await.expect("slatedb put");
    let got = db
        .get(b"hello")
        .await
        .expect("slatedb get")
        .expect("key present after put");
    assert_eq!(got.as_ref(), b"world");
    db.close().await.expect("slatedb close");
}

async fn create_bucket(endpoint: &str, bucket: &str) {
    let creds = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "checkpoint3");
    let cfg = aws_config::defaults(BehaviorVersion::latest())
        .endpoint_url(endpoint)
        .region(REGION)
        .credentials_provider(creds)
        .load()
        .await;
    let s3_cfg = aws_sdk_s3::config::Builder::from(&cfg)
        .force_path_style(true)
        .build();
    let client = aws_sdk_s3::Client::from_conf(s3_cfg);
    client
        .create_bucket()
        .bucket(bucket)
        .send()
        .await
        .expect("create bucket");
}
