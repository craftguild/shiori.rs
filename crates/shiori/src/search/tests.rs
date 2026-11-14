use super::*;
use std::{sync::mpsc, time::Duration};
use tokio::sync::oneshot;

#[test]
fn query_limits_and_literal_cross_field_matching() {
    for text in [
        "".to_owned(),
        " \n\t".to_owned(),
        "日".repeat(257),
        "a ".repeat(17),
    ] {
        assert!(SearchQuery::new(&text).is_err());
    }
    assert!(SearchQuery::new(&"日".repeat(256)).is_ok());
    assert!(SearchQuery::new(&"a ".repeat(16)).is_ok());
    let article = crate::metadata::parse(
        1,
        "---\ntitle: Rust 東京\nauthor: NotSearchable\ndescription: AlsoExcluded\ntags: [Web, 日本語]\n---\n```rust\nPRINTLN!(\"λ\");\n```\nhttps://example.test/path?q=x".as_bytes(),
    ).unwrap();
    for text in [
        "rust println 日本語",
        "東京 web",
        "HTTPS://EXAMPLE.TEST",
        "!(\"λ\");",
        "PATH?Q=X",
    ] {
        assert!(SearchQuery::new(text).unwrap().matches(&article), "{text}");
    }
    for text in [
        "NotSearchable",
        "AlsoExcluded",
        "Rust absent",
        "r.st",
        "東京web",
    ] {
        assert!(!SearchQuery::new(text).unwrap().matches(&article), "{text}");
    }
}

#[tokio::test]
async fn abandoned_workers_keep_their_slot_until_exit_and_observe_cancellation() {
    let executor = Arc::new(SearchExecutor::new());
    let (first_ready, first_started) = oneshot::channel();
    let (release_first, first_gate) = mpsc::channel();
    let owner = executor.clone();
    let first = tokio::spawn(async move {
        owner
            .worker(move |canceled| {
                first_ready.send(canceled).unwrap();
                let _ = first_gate.recv();
            })
            .await
    });
    let canceled = first_started.await.unwrap();
    let (second_ready, second_started) = oneshot::channel();
    let (release_second, second_gate) = mpsc::channel();
    let owner = executor.clone();
    let second = tokio::spawn(async move {
        owner
            .worker(move |_| {
                second_ready.send(()).unwrap();
                let _ = second_gate.recv();
            })
            .await
    });
    second_started.await.unwrap();
    assert_eq!(executor.slots.available_permits(), 0);
    first.abort();
    assert!(first.await.unwrap_err().is_cancelled());
    assert!(canceled.load(Ordering::Relaxed));
    assert_eq!(executor.slots.available_permits(), 0);
    let (third_ready, mut third_started) = oneshot::channel();
    let owner = executor.clone();
    let third =
        tokio::spawn(async move { owner.worker(move |_| third_ready.send(3).unwrap()).await });
    tokio::task::yield_now().await;
    assert!(matches!(
        third_started.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    release_first.send(()).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), third_started)
            .await
            .unwrap()
            .unwrap(),
        3
    );
    third.await.unwrap().unwrap();
    release_second.send(()).unwrap();
    second.await.unwrap().unwrap();
    assert_eq!(executor.slots.available_permits(), 2);
}

#[tokio::test]
async fn canceling_a_queued_search_leaks_no_admission() {
    let executor = Arc::new(SearchExecutor::new());
    let held = executor.slots.clone().acquire_many_owned(2).await.unwrap();
    let owner = executor.clone();
    let queued = tokio::spawn(async move { owner.worker(|_| panic!("canceled queue ran")).await });
    tokio::task::yield_now().await;
    queued.abort();
    assert!(queued.await.unwrap_err().is_cancelled());
    drop(held);
    assert_eq!(executor.worker(|_| 42).await.unwrap(), 42);
    assert_eq!(executor.slots.available_permits(), 2);
}
