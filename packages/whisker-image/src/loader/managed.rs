//! The opt-in URL-keyed cache behind the `cache` prop.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use image::RgbaImage;
use sha2::{Digest, Sha256};

use super::{MAX_DOWNLOAD, fetch};

const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;

type Job = Box<dyn FnOnce(&mut Cache) + Send>;

/// The request the `cache` prop carries: the public options plus the source and headers.
#[derive(Default, serde::Deserialize)]
struct Request {
    #[serde(default, rename = "_src")]
    src: String,
    #[serde(default, rename = "_headers")]
    headers: String,
    #[serde(flatten)]
    bounds: Bounds,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
struct Bounds {
    decode_width: Option<u32>,
    decode_height: Option<u32>,
}

/// Loads an image through the managed cache from the JSON request the `cache` prop carries
///
/// The request holds the [`ImageCacheOptions`](crate::ImageCacheOptions) fields plus
/// `_src` and `_headers`. A memory hit returns the retained decode, a temporary-file
/// hit decodes the saved original, and a miss downloads and saves it first. Only
/// HTTP(S) sources are accepted, and the result never exceeds the decode bounds.
///
/// Loads, [`configure_managed_cache`] and [`remove_managed_image`] run one at a
/// time on a shared worker in call order, and this function blocks until the load
/// finishes, so call it off the UI thread. `wanted` runs when the load reaches the
/// worker; returning `false` skips a request its view no longer shows.
pub fn load_managed(
    request: &str,
    wanted: impl FnOnce() -> bool + Send + 'static,
) -> Result<Arc<RgbaImage>, String> {
    let request: Request = serde_json::from_str(request)
        .map_err(|error| format!("invalid image cache options: {error}"))?;
    if request.src.is_empty() {
        return Err("image URL must not be empty".into());
    }
    decode_size(1, 1, request.bounds)?;
    if !request.src.starts_with("http://") && !request.src.starts_with("https://") {
        return Err("managed image cache requires an HTTP(S) URL".into());
    }
    run(move |cache| {
        if !wanted() {
            return Err("image request was superseded".into());
        }
        cache.load(&request.src, &request.headers, request.bounds)
    })?
}

/// Sets the managed cache's decoded-memory budget from a decimal byte count
///
/// Lowering the budget evicts least recently used decodes immediately, and zero
/// disables retention. Blocks until the shared cache worker applies the change.
pub fn configure_managed_cache(max_bytes: &str) -> Result<(), String> {
    let limit = max_bytes
        .parse::<usize>()
        .map_err(|error| format!("invalid image cache budget: {error}"))?;
    run(move |cache| cache.memory.set_limit(limit))
}

/// Removes the saved original and every retained decode of exactly `url`
///
/// Images already handed to views stay alive through their own references.
/// Blocks until the shared cache worker finishes the removal.
pub fn remove_managed_image(url: &str) -> Result<(), String> {
    let url = url.to_owned();
    run(move |cache| cache.remove(&url))?
}

/// Runs `job` on the worker that owns the shared cache, waiting for its result.
fn run<T: Send + 'static>(job: impl FnOnce(&mut Cache) -> T + Send + 'static) -> Result<T, String> {
    static QUEUE: OnceLock<mpsc::Sender<Job>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("whisker-image-cache".into())
            .spawn(move || {
                let mut cache = Cache::new(DEFAULT_LIMIT, default_root());
                for job in receiver {
                    job(&mut cache);
                }
            })
            .expect("the image cache worker can be spawned");
        sender
    });
    let (reply, result) = mpsc::sync_channel(1);
    queue
        .send(Box::new(move |cache| {
            let _ = reply.send(job(cache));
        }))
        .map_err(|_| "image cache worker stopped".to_owned())?;
    result
        .recv()
        .map_err(|_| "image cache worker stopped".to_owned())
}

/// The OS temporary directory, namespaced by executable so separate apps never share files.
fn default_root() -> PathBuf {
    let identity = std::env::current_exe().unwrap_or_default();
    std::env::temp_dir().join(format!(
        "whisker-images-{}",
        digest(&identity.to_string_lossy())
    ))
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

/// Decoded images keyed by URL digest and bounds, bounded by their total RGBA bytes.
struct MemoryCache {
    entries: lru::LruCache<String, Arc<RgbaImage>>,
    used: usize,
    limit: usize,
}

impl MemoryCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: lru::LruCache::unbounded(),
            used: 0,
            limit,
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<RgbaImage>> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: String, image: Arc<RgbaImage>) {
        if let Some(previous) = self.entries.pop(&key) {
            self.used -= cost(&previous);
        }
        let cost = cost(&image);
        if cost > self.limit {
            return;
        }
        while self.used + cost > self.limit && self.evict() {}
        self.used += cost;
        self.entries.put(key, image);
    }

    fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        while self.used > limit && self.evict() {}
    }

    fn evict(&mut self) -> bool {
        let Some((_, image)) = self.entries.pop_lru() else {
            return false;
        };
        self.used -= cost(&image);
        true
    }

    fn remove_prefix(&mut self, prefix: &str) {
        let keys: Vec<String> = self
            .entries
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, _)| key.clone())
            .collect();
        for key in keys {
            if let Some(image) = self.entries.pop(&key) {
                self.used -= cost(&image);
            }
        }
    }
}

fn cost(image: &RgbaImage) -> usize {
    image.as_raw().len()
}

struct Cache {
    memory: MemoryCache,
    root: PathBuf,
    agent: ureq::Agent,
}

impl Cache {
    fn new(limit: usize, root: PathBuf) -> Self {
        Self {
            memory: MemoryCache::new(limit),
            root,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(60))
                .build(),
        }
    }

    fn load(&mut self, src: &str, headers: &str, bounds: Bounds) -> Result<Arc<RgbaImage>, String> {
        let hash = digest(src);
        let key = format!(
            "{hash}:{:?}:{:?}",
            bounds.decode_width, bounds.decode_height
        );
        if let Some(image) = self.memory.get(&key) {
            return Ok(image);
        }
        let path = self.root.join(&hash);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(image) = decode(&bytes, bounds) {
                self.memory.insert(key, Arc::clone(&image));
                return Ok(image);
            }
            std::fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
        let bytes = fetch(&self.agent, src, headers, MAX_DOWNLOAD + 1)?;
        if bytes.len() as u64 > MAX_DOWNLOAD {
            return Err("image exceeds 64 MiB download limit".into());
        }
        let image = decode(&bytes, bounds)?;
        std::fs::create_dir_all(&self.root).map_err(|error| error.to_string())?;
        let partial = self
            .root
            .join(format!("{hash}.{}.part", std::process::id()));
        let saved =
            std::fs::write(&partial, &bytes).and_then(|()| std::fs::rename(&partial, &path));
        if let Err(error) = saved {
            let _ = std::fs::remove_file(&partial);
            return Err(error.to_string());
        }
        self.memory.insert(key, Arc::clone(&image));
        Ok(image)
    }

    fn remove(&mut self, url: &str) -> Result<(), String> {
        let hash = digest(url);
        self.memory.remove_prefix(&format!("{hash}:"));
        match std::fs::remove_file(self.root.join(hash)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        }
    }
}

fn decode(bytes: &[u8], bounds: Bounds) -> Result<Arc<RgbaImage>, String> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    let orientation = decoder.orientation().map_err(|error| error.to_string())?;
    let mut image =
        image::DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    image.apply_orientation(orientation);
    let (width, height) = decode_size(image.width(), image.height(), bounds)?;
    let image = if (width, height) == (image.width(), image.height()) {
        image
    } else {
        image.resize_exact(width, height, image::imageops::FilterType::Triangle)
    };
    Ok(Arc::new(image.into_rgba8()))
}

/// Scales `width`×`height` down to fit `bounds`, preserving aspect and never enlarging.
fn decode_size(width: u32, height: u32, bounds: Bounds) -> Result<(u32, u32), String> {
    if width == 0
        || height == 0
        || bounds.decode_width == Some(0)
        || bounds.decode_height == Some(0)
    {
        return Err("image and decode dimensions must be positive".into());
    }
    let scale = 1.0_f64
        .min(
            bounds
                .decode_width
                .map_or(1.0, |bound| f64::from(bound) / f64::from(width)),
        )
        .min(
            bounds
                .decode_height
                .map_or(1.0, |bound| f64::from(bound) / f64::from(height)),
        );
    Ok((
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(RgbaImage::new(width, height))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        png.into_inner()
    }

    fn bounds(decode_width: Option<u32>, decode_height: Option<u32>) -> Bounds {
        Bounds {
            decode_width,
            decode_height,
        }
    }

    #[test]
    fn byte_budget_evicts_least_recently_used_and_can_shrink() {
        let mut cache = MemoryCache::new(32);
        let image = Arc::new(RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])));
        cache.insert("a".into(), image.clone());
        cache.insert("b".into(), image.clone());
        assert!(cache.get("a").is_some());
        cache.insert("c".into(), image.clone());
        assert!(cache.get("b").is_none());
        assert!(cache.get("a").is_some());
        assert_eq!(cache.used, 32);
        cache.set_limit(16);
        assert!(cache.get("c").is_none());
        cache.remove_prefix("a");
        assert_eq!(cache.used, 0);
        cache.set_limit(0);
        cache.insert("oversized".into(), image);
        assert_eq!(cache.used, 0);
    }

    #[test]
    fn decode_dimensions_preserve_aspect_and_never_upscale() {
        assert_eq!(
            decode_size(800, 600, bounds(Some(400), Some(400))).unwrap(),
            (400, 300)
        );
        assert_eq!(
            decode_size(800, 600, bounds(None, Some(150))).unwrap(),
            (200, 150)
        );
        assert_eq!(
            decode_size(80, 60, bounds(Some(400), None)).unwrap(),
            (80, 60)
        );
        assert!(decode_size(800, 600, bounds(Some(0), None)).is_err());
    }

    #[test]
    fn disk_survives_memory_eviction_and_corruption_refetches_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/image?variant=1", listener.local_addr().unwrap());
        let bytes = png(8, 6);
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 2048];
                assert!(stream.read(&mut request).unwrap() > 0);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                )
                .unwrap();
                stream.write_all(&bytes).unwrap();
            }
        });
        let root =
            std::env::temp_dir().join(format!("whisker-managed-test-{}", std::process::id()));
        let mut cache = Cache::new(1024, root.clone());
        let sized = bounds(Some(4), None);
        let first = cache.load(&url, "", sized).unwrap();
        assert_eq!(first.dimensions(), (4, 3));
        assert!(Arc::ptr_eq(&first, &cache.load(&url, "", sized).unwrap()));
        cache.memory.set_limit(0);
        let mut reopened = Cache::new(0, root.clone());
        assert_eq!(
            reopened
                .load(&url, "", Bounds::default())
                .unwrap()
                .dimensions(),
            (8, 6)
        );
        std::fs::write(root.join(digest(&url)), b"broken").unwrap();
        assert_eq!(reopened.load(&url, "", sized).unwrap().dimensions(), (4, 3));
        server.join().unwrap();
        assert_ne!(digest(&url), digest(&url.replace("variant=1", "variant=2")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_worker_loads_requests_in_order_and_removes_them() {
        let url = format!("https://example.com/managed-{}", std::process::id());
        let root = default_root();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(digest(&url)), png(8, 6)).unwrap();
        let request = |width: u32| {
            serde_json::json!({"_src": url, "_headers": "", "decode_width": width}).to_string()
        };
        assert_eq!(
            load_managed(&request(8), || true).unwrap().dimensions(),
            (8, 6)
        );
        let sized = load_managed(&request(4), || true).unwrap();
        assert_eq!(sized.dimensions(), (4, 3));
        assert!(Arc::ptr_eq(
            &sized,
            &load_managed(&request(4), || true).unwrap()
        ));
        assert!(load_managed(&request(4), || false).is_err());
        assert!(load_managed(&request(0), || true).is_err());
        assert!(load_managed(r#"{"_src": "file:///tmp/a.png"}"#, || true).is_err());
        configure_managed_cache("0").unwrap();
        assert!(configure_managed_cache("-1").is_err());
        remove_managed_image(&url).unwrap();
        assert!(!root.join(digest(&url)).exists());
        configure_managed_cache(&DEFAULT_LIMIT.to_string()).unwrap();
    }
}
