use crate::infrastructure::filesystem::create_parent_dir;

use std::{borrow::Cow, cmp::min, fs::File, io::Write, path::Path, time::Duration};

use anyhow::{Context, Result};
use colored::Colorize;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use tokio_retry::{
    strategy::{jitter, ExponentialBackoff},
    Retry,
};
use truncatable::Truncatable;

/// Total number of retries attempted on top of the initial request.
pub const MAX_RETRIES: usize = 3;
pub const DETAIL_PREFIX: &str = "   ";
pub const MIHOTO_GITHUB_MIRROR_ENV: &str = "MIHOTO_GITHUB_MIRROR";

/// Shared retry strategy for HTTP operations.
///
/// Yields up to [`MAX_RETRIES`] retries (so up to `MAX_RETRIES + 1` total attempts) with
/// exponential backoff of ~1s, ~2s, ~4s, each with jitter and capped at 5s.
/// `ExponentialBackoff::from_millis(2).factor(500)` seeds `current = 2` and multiplies by
/// `base = 2` each step, so the yielded delays are `2 * 500`, `4 * 500`, `8 * 500`, ... ms
/// before jitter.
pub fn retry_strategy() -> impl Iterator<Item = Duration> {
    ExponentialBackoff::from_millis(2)
        .factor(500)
        .max_delay(Duration::from_secs(5))
        .map(jitter)
        .take(MAX_RETRIES)
}

fn github_mirror_base() -> Option<String> {
    let mirror = std::env::var(MIHOTO_GITHUB_MIRROR_ENV).ok()?;
    let mirror = mirror.trim().trim_end_matches('/').to_string();
    if mirror.is_empty() {
        return None;
    }
    Some(mirror)
}

fn is_github_download_host(host: &str) -> bool {
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

/// Prefix GitHub-hosted download urls with the configured mirror, if any.
///
/// This intentionally excludes `api.github.com` so API metadata requests continue to use
/// GitHub directly while large artifact downloads can still flow through a mirror.
pub fn resolve_download_url(url: &str) -> Cow<'_, str> {
    let Some(mirror) = github_mirror_base() else {
        return Cow::Borrowed(url);
    };

    let Ok(parsed) = reqwest::Url::parse(url) else {
        return Cow::Borrowed(url);
    };

    let Some(host) = parsed.host_str() else {
        return Cow::Borrowed(url);
    };

    if !is_github_download_host(host) {
        return Cow::Borrowed(url);
    }

    if url == mirror || url.starts_with(&format!("{mirror}/")) {
        return Cow::Borrowed(url);
    }

    Cow::Owned(format!("{mirror}/{url}"))
}

/// Download file from url to path with a reusable http client.
///
/// Performs the initial request, then retries up to [`MAX_RETRIES`] more times on any
/// failure (connection, HTTP status, stream, or IO error). Each attempt truncates the
/// destination file.
pub async fn download_file(
    client: &Client,
    url: &str,
    path: &Path,
    user_agent: &str,
) -> Result<()> {
    let mut attempt = 0usize;
    Retry::spawn(retry_strategy(), || {
        // attempt = 0 is the initial request; retries are 1..=MAX_RETRIES.
        let retry_no = attempt;
        attempt += 1;
        async move {
            if retry_no > 0 {
                println!(
                    "{} Retrying download (attempt {}/{})...",
                    DETAIL_PREFIX.yellow(),
                    retry_no,
                    MAX_RETRIES
                );
            }
            download_file_once(client, url, path, user_agent).await
        }
    })
    .await
}

/// Single-shot download with progress bar. Called by [`download_file`] on each retry.
///
/// Renders a progress bar if content-length is available from the url headers provided. If not,
/// renders a spinner to indicate that something is downloading. On failure the bar is cleared so
/// the next retry renders cleanly.
///
/// With reference from:
/// * https://github.com/mihaigalos/tutorials/blob/800d5acbc333fd4068622e9b3d870cb5b7d34e12/rust/download_with_progressbar/src/main.rs
/// * https://github.com/console-rs/indicatif/blob/2954b1a24ac5f1900a7861992e4825bff643c9e2/examples/yarnish.rs
///
/// Note: Allow `clippy::unused_io_amount` because we are writing downloaded chunks on the fly.
#[allow(clippy::unused_io_amount)]
async fn download_file_once(
    client: &Client,
    url: &str,
    path: &Path,
    user_agent: &str,
) -> Result<()> {
    let resolved_url = resolve_download_url(url);

    // Create parent directory for download destination if not exists
    create_parent_dir(path)?;

    // Create shared http client for multiple downloads when possible
    let res = client
        .get(resolved_url.as_ref())
        .header("User-Agent", user_agent)
        .send()
        .await
        .with_context(|| format!("failed to GET from '{}'", resolved_url.as_ref()))?;
    res.error_for_status_ref()?;

    // If content length is not available or 0, use a spinner instead of a progress bar
    let total_size = res.content_length().unwrap_or(0);
    let pb = ProgressBar::new(total_size);

    let bar_style = ProgressStyle::with_template(
        "{prefix:.cyan} Downloading {msg}\n{prefix:.cyan} {elapsed_precise} \
         [{bar:30.white/cyan}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})",
    )?
    .progress_chars("-> ");
    let spinner_style = ProgressStyle::with_template(
        "{prefix:.cyan} Downloading {wide_msg}\n{prefix:.cyan} \
         {spinner} {elapsed_precise} \u{2014} {bytes_per_sec}",
    )?;

    if total_size == 0 {
        pb.set_style(spinner_style);
    } else {
        pb.set_style(bar_style);
    }
    pb.set_prefix(DETAIL_PREFIX);

    let truncated_url = Truncatable::from(url)
        .truncator("...".into())
        .truncate(64)
        .underline();
    pb.set_message(format!("{truncated_url}"));

    // Perform the streamed write in a scoped async block so we can clean up the progress bar
    // regardless of success or failure.
    let result: Result<()> = async {
        let mut file = File::create(path)?;
        let mut downloaded: u64 = 0;
        let mut stream = res.bytes_stream();

        while let Some(item) = stream.next().await {
            let chunk = item.with_context(|| "error while downloading file")?;

            file.write(&chunk)
                .with_context(|| "error while writing to file")?;
            if total_size != 0 {
                let new = min(downloaded + (chunk.len() as u64), total_size);
                downloaded = new;
                pb.set_position(new);
            } else {
                pb.inc(chunk.len() as u64);
            }
        }
        Ok(())
    }
    .await;

    match &result {
        Ok(()) => {
            // Clear the progress bar and print a single summary line so the output
            // stays visually aligned inside the stage body output.
            pb.finish_and_clear();
            println!(
                "{} Downloaded to {}",
                DETAIL_PREFIX.cyan(),
                path.to_str().unwrap_or("").underline()
            );
        }
        // Clear the bar before the outer retry loop prints its next message.
        Err(_) => pb.finish_and_clear(),
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn mirror_applies_only_to_github_download_hosts() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(MIHOTO_GITHUB_MIRROR_ENV, "https://gh-proxy.org/");
        assert_eq!(
            resolve_download_url("https://github.com/example/file.tar.gz").as_ref(),
            "https://gh-proxy.org/https://github.com/example/file.tar.gz"
        );
        assert_eq!(
            resolve_download_url("https://api.github.com/repos/example/releases").as_ref(),
            "https://api.github.com/repos/example/releases"
        );
        assert_eq!(
            resolve_download_url("https://example.com/file.tar.gz").as_ref(),
            "https://example.com/file.tar.gz"
        );
        std::env::remove_var(MIHOTO_GITHUB_MIRROR_ENV);
    }
}
