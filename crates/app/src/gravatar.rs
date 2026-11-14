use gpui_kit::{App, AsyncApp, Global, Image, ImageFormat, RenderImage, SvgRenderer};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

const MAX_IMAGE_BYTES: u64 = 256 * 1024;
const REFRESH_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
struct Avatars(HashMap<String, Avatar>);

impl Global for Avatars {}

#[derive(Default)]
struct Avatar {
    image: Option<Arc<RenderImage>>,
    refresh_at: Option<Instant>,
}

type AvatarImage = Option<Arc<RenderImage>>;

pub(crate) fn image(email: &str, cx: &mut App) -> Option<Arc<RenderImage>> {
    let hash = gravatar_hash(email)?;
    let entries = &mut cx.default_global::<Avatars>().0;
    let mut read_disk = false;
    let entry = entries.entry(hash.clone()).or_insert_with(|| {
        read_disk = true;
        Avatar {
            refresh_at: Some(Instant::now()),
            ..Avatar::default()
        }
    });
    let image = entry.image.clone();
    if entry.refresh_at.is_some_and(|at| Instant::now() >= at) {
        entry.refresh_at = None;
        let renderer = cx.svg_renderer();
        cx.spawn(async move |cx| {
            let (path, cached) = smol::unblock({
                let hash = hash.clone();
                let renderer = renderer.clone();
                move || {
                    let path = cairn_core::paths::data_dir()
                        .join("cache")
                        .join("gravatar")
                        .join(hash);
                    let cached = read_disk.then(|| read_cached(&path, &renderer)).flatten();
                    (path, cached)
                }
            })
            .await;

            if let Some((image, remaining)) = cached {
                publish(cx, &hash, Some(image), remaining);
                if !remaining.is_zero() {
                    redraw_after(cx, remaining).await;
                    return;
                }
            }

            let request_hash = hash.clone();
            let result = smol::unblock(move || download(&request_hash, &path, &renderer)).await;
            let (image, delay) = match result {
                Some(image) => (Some(image), REFRESH_INTERVAL),
                None => (None, RETRY_INTERVAL),
            };
            publish(cx, &hash, image, delay);
            redraw_after(cx, delay).await;
        })
        .detach();
    }
    image
}

// An absent result means a transient error, so leave the last image in place.
fn publish(cx: &mut AsyncApp, hash: &str, image: Option<AvatarImage>, delay: Duration) {
    cx.update(|cx| {
        let entry = cx
            .global_mut::<Avatars>()
            .0
            .get_mut(hash)
            .expect("requested avatar");
        if let Some(image) = image {
            entry.image = image;
        }
        entry.refresh_at = (!delay.is_zero()).then(|| Instant::now() + delay);
        cx.refresh_windows();
    });
}

async fn redraw_after(cx: &mut AsyncApp, delay: Duration) {
    cx.background_executor().timer(delay).await;
    cx.update(|cx| cx.refresh_windows());
}

fn gravatar_hash(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    let (local, domain) = email.split_once('@')?;
    if local.is_empty() || domain.is_empty() {
        return None;
    }
    Some(format!("{:x}", Sha256::digest(email.as_bytes())))
}

fn read_cached(path: &Path, renderer: &SvgRenderer) -> Option<(AvatarImage, Duration)> {
    let file = std::fs::File::open(path).ok()?;
    let modified = file.metadata().ok()?.modified().ok()?;
    let remaining = REFRESH_INTERVAL.saturating_sub(modified.elapsed().unwrap_or_default());
    let mut record = Vec::new();
    file.take(MAX_IMAGE_BYTES + 65)
        .read_to_end(&mut record)
        .ok()?;
    if record.is_empty() {
        return Some((None, remaining)); // An empty record represents a Gravatar 404.
    }
    let separator = record.iter().position(|&byte| byte == b'\n')?;
    let mime = std::str::from_utf8(&record[..separator]).ok()?;
    let format = image_format(mime)?;
    let bytes = record.split_off(separator + 1);
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return None;
    }
    let image = Image::from_bytes(format, bytes)
        .to_image_data(renderer.clone())
        .ok()?;
    Some((Some(image), remaining))
}

fn download(hash: &str, path: &Path, renderer: &SvgRenderer) -> Option<AvatarImage> {
    static CLIENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .build()
            .into()
    });

    let url = format!("https://gravatar.com/avatar/{hash}?s=96&d=404");
    let mut response = match CLIENT.get(&url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => {
            let _ = write_cached(path, None);
            return Some(None);
        }
        Err(_) => return None,
    };
    let content_type = response.headers().get("content-type")?.to_str().ok()?;
    let format = image_format(content_type)?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_IMAGE_BYTES)
        .read_to_vec()
        .ok()?;
    let image = Image::from_bytes(format, bytes.clone())
        .to_image_data(renderer.clone())
        .ok()?;
    let _ = write_cached(path, Some((format, &bytes)));
    Some(Some(image))
}

fn image_format(mime: &str) -> Option<ImageFormat> {
    let format = ImageFormat::from_mime_type(mime)?;
    matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::Webp
    )
    .then_some(format)
}

fn write_cached(path: &Path, image: Option<(ImageFormat, &[u8])>) -> io::Result<()> {
    let parent = path.parent().expect("avatar cache directory");
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    if let Some((format, bytes)) = image {
        writeln!(file, "{}", format.mime_type())?;
        file.write_all(bytes)?;
    }
    file.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_normalized_gravatar_email() {
        assert_eq!(
            gravatar_hash("  USER@example.COM ").as_deref(),
            Some("b4c9a289323b21a01c3e940f150eb9b8c542587f1abfd8f0e1cc1ffc5e475514"),
        );
        assert_eq!(gravatar_hash(" "), None);
        assert_eq!(gravatar_hash("user@"), None);
    }

    #[test]
    fn reads_stale_images_and_overwrites_them_with_negative_results() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gravatar").join("author");
        let renderer = SvgRenderer::new(Arc::new(()));
        let png = include_bytes!("../assets/cairn.png");

        write_cached(&path, Some((ImageFormat::Png, png))).unwrap();
        let (image, remaining) = read_cached(&path, &renderer).unwrap();
        assert!(image.is_some());
        assert!(!remaining.is_zero());

        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_modified(std::time::SystemTime::now() - REFRESH_INTERVAL)
            .unwrap();
        drop(file);
        let (image, remaining) = read_cached(&path, &renderer).unwrap();
        assert!(image.is_some());
        assert!(remaining.is_zero());

        write_cached(&path, None).unwrap();
        let (image, remaining) = read_cached(&path, &renderer).unwrap();
        assert!(image.is_none());
        assert!(!remaining.is_zero());
    }
}
