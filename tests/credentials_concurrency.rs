use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use atlassian_cli::auth::{KeychainAccess, TokenSet, TokenStore, remove_credentials_file};
use secrecy::SecretString;

fn store(path: &Path, profile: &str) -> TokenStore {
    TokenStore::at(profile, path.to_path_buf(), KeychainAccess::Forbidden)
}

fn tokens() -> TokenSet {
    TokenSet {
        access_token: SecretString::new("fixture-access".into()),
        refresh_token: None,
        expires_at_unix: 4_102_444_800,
        scopes: Vec::new(),
        cloud_id: None,
    }
}

#[test]
#[ignore = "subprocess fixture for concurrent_processes_preserve_profiles"]
fn credentials_worker() {
    let path = std::env::var_os("ATLASSIAN_TEST_CREDENTIALS_PATH").unwrap();
    let profile = std::env::var("ATLASSIAN_TEST_CREDENTIALS_PROFILE").unwrap();
    let operation = std::env::var("ATLASSIAN_TEST_CREDENTIALS_OPERATION").unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    println!("READY");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    assert_ne!(std::io::stdin().read_line(&mut line).unwrap(), 0);
    runtime.block_on(async {
        let store = store(Path::new(&path), &profile);
        match operation.as_str() {
            "save" => {
                store.save(&tokens()).await.unwrap();
            }
            "delete" => store.delete().await.unwrap(),
            _ => panic!("unknown fixture operation"),
        }
    });
}

#[tokio::test]
async fn concurrent_processes_preserve_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    let count = 8;
    store(&path, "retained").save(&tokens()).await.unwrap();
    for index in 0..count {
        store(&path, &format!("delete-{index}"))
            .save(&tokens())
            .await
            .unwrap();
    }

    let mut children = Vec::new();
    for operation in ["save", "delete"] {
        for index in 0..count {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "credentials_worker",
                    "--ignored",
                    "--nocapture",
                    "--format=terse",
                ])
                .env("ATLASSIAN_TEST_CREDENTIALS_PATH", &path)
                .env(
                    "ATLASSIAN_TEST_CREDENTIALS_PROFILE",
                    format!("{operation}-{index}"),
                )
                .env("ATLASSIAN_TEST_CREDENTIALS_OPERATION", operation)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let stdout = child.stdout.take().unwrap();
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                assert_ne!(
                    reader.read_line(&mut line).unwrap(),
                    0,
                    "worker exited before ready"
                );
                if line.trim() == "READY" {
                    break;
                }
            }
            children.push((child, reader));
        }
    }
    for (child, _) in &mut children {
        writeln!(child.stdin.take().unwrap(), "GO").unwrap();
    }
    for (mut child, _output) in children {
        assert!(child.wait().unwrap().success());
    }

    assert!(store(&path, "retained").load().await.unwrap().is_some());
    for index in 0..count {
        assert!(
            store(&path, &format!("save-{index}"))
                .load()
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store(&path, &format!("delete-{index}"))
                .load()
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[tokio::test]
async fn contended_mutations_leave_the_reactor_available() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("credentials.json");
    for operation in ["save", "delete", "remove"] {
        store(&path, "existing").save(&tokens()).await.unwrap();
        let lock = std::fs::File::options()
            .read(true)
            .write(true)
            .open(dir.path().join("credentials.json.lock"))
            .unwrap();
        lock.lock().unwrap();
        let worker_path = path.clone();
        let mut worker = tokio::spawn(async move {
            match operation {
                "save" => {
                    store(&worker_path, "new").save(&tokens()).await.unwrap();
                }
                "delete" => store(&worker_path, "existing").delete().await.unwrap(),
                "remove" => {
                    tokio::task::spawn_blocking(move || remove_credentials_file(&worker_path))
                        .await
                        .unwrap()
                        .unwrap();
                }
                _ => unreachable!(),
            }
        });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut worker)
                .await
                .is_err()
        );
        drop(lock);
        tokio::time::timeout(Duration::from_secs(5), worker)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store(&path, "existing").load().await.unwrap().is_some(),
            operation == "save"
        );
        if operation == "save" {
            assert!(store(&path, "new").load().await.unwrap().is_some());
        }
    }
}
