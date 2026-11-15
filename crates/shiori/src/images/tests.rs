use super::*;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::{
    io::Cursor,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) struct Probe {
    event: &'static str,
    fired: AtomicBool,
    pub entered: tokio::sync::Notify,
    release: Mutex<bool>,
    ready: Condvar,
    panic: bool,
}
impl Probe {
    pub fn new(event: &'static str, panic: bool) -> Arc<Self> {
        Arc::new(Self {
            event,
            fired: AtomicBool::new(false),
            entered: tokio::sync::Notify::new(),
            release: Mutex::new(false),
            ready: Condvar::new(),
            panic,
        })
    }
    pub fn wait(&self, event: &str) {
        if event != self.event || self.fired.swap(true, Ordering::SeqCst) {
            return;
        }
        self.entered.notify_one();
        let mut released = self.release.lock().unwrap();
        while !*released {
            released = self.ready.wait(released).unwrap();
        }
        drop(released);
        assert!(!self.panic, "injected real worker panic");
    }
    pub fn release(&self) {
        *self.release.lock().unwrap() = true;
        self.ready.notify_all();
    }
}
fn image(width: u32, height: u32, value: u8) -> Vec<u8> {
    let pixels = RgbaImage::from_pixel(width, height, Rgba([value, 30, 40, 255]));
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(pixels)
        .write_to(&mut out, ImageFormat::Png)
        .unwrap();
    out.into_inner()
}
fn service(workers: usize, budget: usize) -> (tempfile::TempDir, Arc<ImageService>) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    for value in 1..=3 {
        std::fs::write(
            root.path().join(format!("assets/{value}.png")),
            image(80, 40, value),
        )
        .unwrap();
    }
    let service = ImageService::configured(
        Arc::new(crate::storage::root(&root.path().join("assets")).unwrap()),
        workers,
        budget,
    );
    (root, service)
}
async fn variant(service: &Arc<ImageService>, name: &str) -> Result<Arc<Variant>, Error> {
    service
        .variant(
            AssetPath::parse(name).unwrap(),
            Width::Auto,
            Some("image/webp".into()),
        )
        .await
}
async fn until(condition: impl Fn() -> bool) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn coalesced_work_and_permits_outlive_cancelled_owner_and_waiter() {
    let (_root, service) = service(2, 1_000_000);
    let gate = Probe::new("encode", false);
    *service.probe.lock().unwrap() = Some(gate.clone());
    let first = service.clone();
    let owner = tokio::spawn(async move { variant(&first, "1.png").await });
    gate.entered.notified().await;
    let second = service.clone();
    let waiter = tokio::spawn(async move { variant(&second, "1.png").await });
    until(|| service.flight_waiters.load(Ordering::Relaxed) == 1).await;
    owner.abort();
    waiter.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert_eq!(service.available_workers(), 0);
    assert_eq!(service.active_keys(), 1);
    assert_eq!(service.transforms(), 1);
    gate.release();
    until(|| service.available_workers() == 2 && service.active_keys() == 0).await;
    let first = variant(&service, "1.png").await.unwrap();
    let second = variant(&service, "1.png").await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(service.transforms(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admission_is_bounded_before_reads_and_cancelled_queue_shutdown_and_panic_clean_up() {
    let (_root, service) = service(1, 1_000_000);
    let gate = Probe::new("encode", true);
    *service.probe.lock().unwrap() = Some(gate.clone());
    let first = service.clone();
    let owner = tokio::spawn(async move { variant(&first, "1.png").await });
    gate.entered.notified().await;
    let mut queued = Vec::new();
    for _ in 0..64 {
        let service = service.clone();
        queued.push(tokio::spawn(
            async move { variant(&service, "2.png").await },
        ));
    }
    until(|| service.queued() == 64).await;
    assert_eq!(variant(&service, "3.png").await.unwrap_err(), Error::Busy);
    assert_eq!(service.transforms(), 1);
    for job in queued {
        job.abort();
        assert!(job.await.unwrap_err().is_cancelled());
    }
    until(|| service.queued() == 0).await;
    gate.release();
    assert_eq!(owner.await.unwrap().unwrap_err(), Error::Internal);
    until(|| service.available_workers() == 1 && service.active_keys() == 0).await;
    assert!(
        variant(&service, "1.png").await.is_ok(),
        "a real codec succeeds after the injected panic"
    );
    service.begin_shutdown();
    assert_eq!(variant(&service, "1.png").await.unwrap_err(), Error::Busy);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inspection_also_keeps_admission_until_actual_work_finishes() {
    let (_root, service) = service(1, 1_000_000);
    let gate = Probe::new("inspected", false);
    *service.probe.lock().unwrap() = Some(gate.clone());
    let first = service.clone();
    let job = tokio::spawn(async move { first.inspect(AssetPath::parse("1.png").unwrap()).await });
    gate.entered.notified().await;
    job.abort();
    assert!(job.await.unwrap_err().is_cancelled());
    assert_eq!(service.available_workers(), 0);
    gate.release();
    until(|| service.available_workers() == 1).await;
}

#[tokio::test]
async fn byte_lru_reuses_shared_buffers_evicts_and_never_caches_a_failure() {
    let (root, service) = service(1, 1_000_000);
    let mut max_charge = 0;
    for name in ["1.png", "2.png", "3.png"] {
        let value = variant(&service, name).await.unwrap();
        max_charge = max_charge.max(
            value.bytes.len()
                + value.etag.capacity()
                + std::mem::size_of::<Variant>()
                + std::mem::size_of::<cache::Key>()
                + 128,
        );
    }
    let service = ImageService::configured(
        Arc::new(crate::storage::root(&root.path().join("assets")).unwrap()),
        1,
        max_charge * 2,
    );
    variant(&service, "1.png").await.unwrap();
    variant(&service, "2.png").await.unwrap();
    variant(&service, "1.png").await.unwrap();
    variant(&service, "3.png").await.unwrap();
    assert_eq!(service.transforms(), 3);
    assert!(service.cache_bytes() <= max_charge * 2);
    variant(&service, "2.png").await.unwrap();
    assert_eq!(service.transforms(), 4);
    std::fs::write(
        root.path().join("assets/broken"),
        b"\x89PNG\r\n\x1a\nbroken",
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            variant(&service, "broken").await.unwrap_err(),
            Error::Decode
        );
    }
    assert_eq!(service.active_keys(), 0);
    assert_eq!(service.available_workers(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn image_change_during_html_preparation_returns_originals_and_does_not_cache_stale_candidates()
 {
    use crate::{
        catalog::Catalog,
        config::Config,
        rendering::{Renderer, RequestMemo},
        storage,
    };
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("articles")).unwrap();
    std::fs::create_dir_all(root.path().join("assets")).unwrap();
    std::fs::write(
        root.path().join("articles/1.md"),
        "---\ntitle: Live image\n---\n![A](/assets/p.png)",
    )
    .unwrap();
    std::fs::write(root.path().join("assets/p.png"), image(10, 20, 1)).unwrap();
    let config = Arc::new(
        Config::new(
            root.path(),
            "127.0.0.1:0".parse().unwrap(),
            "https://cms.example/blog/",
        )
        .unwrap(),
    );
    let catalog = Catalog::open(storage::root(root.path()).unwrap())
        .await
        .unwrap();
    let renderer = Renderer::new(config, &catalog);
    let article = catalog.snapshot().articles["1"].clone();
    let gate = Probe::new("prepared", false);
    *renderer.images.probe.lock().unwrap() = Some(gate.clone());
    let first = renderer.clone();
    let initial = article.clone();
    let job = tokio::spawn(async move { first.render(initial, &RequestMemo::default()).await });
    gate.entered.notified().await;
    std::fs::write(root.path().join("assets/p.png"), image(30, 40, 2)).unwrap();
    gate.release();
    let stale = job.await.unwrap().unwrap();
    let html = &stale.blocks[0].html;
    assert!(html.contains("/assets/p.png"));
    assert!(!html.contains("srcset"));
    assert!(!stale.cacheable);
    let current = renderer
        .render(article.clone(), &RequestMemo::default())
        .await
        .unwrap();
    assert!(current.blocks[0].html.contains(" 30w"));
    assert!(current.cacheable);
    assert_eq!(renderer.calls(), 2);
    renderer
        .render(article, &RequestMemo::default())
        .await
        .unwrap();
    assert_eq!(renderer.calls(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_rejects_queued_work_but_running_work_keeps_its_permit() {
    let (_root, service) = service(1, 1_000_000);
    let gate = Probe::new("encode", false);
    *service.probe.lock().unwrap() = Some(gate.clone());
    let first = service.clone();
    let owner = tokio::spawn(async move { variant(&first, "1.png").await });
    gate.entered.notified().await;
    let second = service.clone();
    let waiter = tokio::spawn(async move { variant(&second, "2.png").await });
    until(|| service.queued() == 1).await;
    service.begin_shutdown();
    assert_eq!(waiter.await.unwrap().unwrap_err(), Error::Busy);
    assert_eq!(service.queued(), 0);
    assert_eq!(service.active_keys(), 1);
    assert_eq!(service.available_workers(), 0);
    gate.release();
    assert!(owner.await.unwrap().is_ok());
    until(|| service.active_keys() == 0).await;
}

#[tokio::test]
async fn corrupt_pixel_stream_errors_after_flight_creation_are_not_retained() {
    use img_parts::png::{Png, PngChunk};
    let (root, service) = service(1, 1_000_000);
    let mut container = Png::from_bytes(image(80, 40, 1).into()).unwrap();
    let idat = container
        .chunks_mut()
        .iter_mut()
        .find(|c| c.kind() == *b"IDAT")
        .unwrap();
    let data = vec![0x78, 0x9c, 0x07, 0xff, 0xff];
    *idat = PngChunk::new(*b"IDAT", data.into());
    let bytes = container.encoder().bytes().to_vec();
    assert!(
        codec::header(&bytes).is_ok(),
        "real header succeeds before pixel failure"
    );
    std::fs::write(root.path().join("assets/corrupt"), bytes).unwrap();
    let before = service.cache_bytes();
    for count in 1..=2 {
        assert_eq!(
            variant(&service, "corrupt").await.unwrap_err(),
            Error::Decode
        );
        assert_eq!(service.transforms(), count);
        assert_eq!(service.active_keys(), 0);
        assert_eq!(service.available_workers(), 1);
        assert_eq!(service.cache_bytes(), before);
    }
}

#[test]
fn actual_codec_output_limits_retain_acceptable_unscaled_originals_and_reject_resized_overflow() {
    let mut original = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut original, 64, 64);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&vec![0u8; 64 * 64 * 3]).unwrap();
        writer.finish().unwrap();
    }
    let bytes = original.clone();
    let budget = bytes.len();
    let decoded = codec::decode(&bytes).unwrap();
    let retained = decoded
        .encode_with_limit(original, 64, Format::Png, budget)
        .unwrap();
    assert_eq!(retained.bytes.as_ref(), bytes);
    assert_eq!(
        codec::decode(&bytes)
            .unwrap()
            .encode_with_limit(bytes.clone(), 32, Format::Png, 8)
            .unwrap_err(),
        Error::OutputLimit
    );
}
