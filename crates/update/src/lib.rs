//! Download and verify official Windows installers before handing them to the application.

#[cfg(windows)]
mod winhttp;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const LATEST_RELEASE: &str = "https://api.github.com/repos/zhkai-ybwn/fishing/releases/latest";
const RELEASE_DOWNLOADS: &str = "https://github.com/zhkai-ybwn/fishing/releases/download/";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    pub notes: String,
    installer: Installer,
}

#[derive(Clone, Debug)]
struct Installer {
    name: String,
    url: Url,
    size: u64,
    sha256: [u8; 32],
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Deserialize)]
struct ReleaseAsset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

/// Check the official public GitHub repository for a newer stable release.
pub fn check(current: &str) -> Result<Option<Update>, String> {
    let current = version(current).ok_or("当前程序的版本号无效。")?;
    let url = Url::parse(LATEST_RELEASE).expect("the release API URL is valid");
    let response = get(&url, RequestKind::Metadata, CHECK_TIMEOUT)
        .map_err(|error| format!("无法连接 GitHub 检查更新：{error}"))?;
    match response.status {
        200 => {}
        404 => return Ok(None),
        403 | 429 => return Err("GitHub 暂时限制了更新查询，请稍后重试。".to_owned()),
        status => return Err(format!("GitHub 更新查询返回 HTTP {status}。")),
    }
    let mut bytes = Vec::new();
    response
        .body
        .take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("无法读取更新信息：{error}"))?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err("GitHub 返回的更新信息过大。".to_owned());
    }
    parse_release(current, &bytes)
}

fn parse_release(current: [u32; 3], bytes: &[u8]) -> Result<Option<Update>, String> {
    let release: Release =
        serde_json::from_slice(bytes).map_err(|_| "GitHub 返回的更新信息格式无效。")?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let tag_version = release
        .tag_name
        .strip_prefix('v')
        .ok_or("发布标签的版本号无效。")?;
    let latest = version(tag_version).ok_or("发布标签的版本号无效。")?;
    if latest <= current {
        return Ok(None);
    }

    let name = format!("fishing-{tag_version}-windows-x64-setup.exe");
    let mut assets = release
        .assets
        .into_iter()
        .filter(|asset| asset.name == name);
    let asset = assets.next().ok_or("新版本尚未提供 Windows x64 安装包。")?;
    if assets.next().is_some() || asset.state != "uploaded" {
        return Err("新版本的安装包尚未准备好。".to_owned());
    }
    if asset.size == 0 || asset.size > MAX_INSTALLER_BYTES {
        return Err("新版本的安装包大小无效。".to_owned());
    }
    let expected_url = format!("{RELEASE_DOWNLOADS}{}/{name}", release.tag_name);
    if asset.browser_download_url != expected_url {
        return Err("更新包的下载地址与官方仓库不符。".to_owned());
    }
    let url =
        Url::parse(&expected_url).expect("the release URL is built from validated version digits");
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(parse_sha256)
        .ok_or("GitHub 尚未提供有效的安装包 SHA-256 校验值。")?;
    let notes = release
        .body
        .unwrap_or_default()
        .chars()
        .take(8000)
        .collect();
    Ok(Some(Update {
        version: tag_version.to_owned(),
        notes,
        installer: Installer {
            name,
            url,
            size: asset.size,
            sha256,
        },
    }))
}

fn version(value: &str) -> Option<[u32; 3]> {
    let mut parts = value.split('.');
    let mut version = [0; 3];
    for number in &mut version {
        let part = parts.next()?;
        if part.is_empty()
            || part.len() > 1 && part.starts_with('0')
            || !part.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

fn parse_sha256(value: &str) -> Option<[u8; 32]> {
    let hex = value.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut hash = [0; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(hash)
}

/// Save the installer to the application's per-user update directory and verify the returned file.
pub fn download(update: &Update, directory: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|error| format!("无法创建更新目录：{error}"))?;
    let path = std::path::absolute(directory.join(&update.installer.name))
        .map_err(|error| format!("无法确定更新包的保存位置：{error}"))?;
    if verify_file(&path, &update.installer).is_ok() {
        return Ok(path);
    }

    let response = get(
        &update.installer.url,
        RequestKind::Installer,
        DOWNLOAD_TIMEOUT,
    )
    .map_err(|error| format!("无法下载安装包：{error}"))?;
    if response.status != 200 {
        return Err(format!("GitHub 安装包下载返回 HTTP {}。", response.status));
    }
    save_installer(response.body, &path, &update.installer)?;
    Ok(path)
}

/// Revalidate the package immediately before launching it.
/// Keep the returned read handle open through process creation on Windows.
pub fn verified_installer(update: &Update, path: &Path) -> Result<File, String> {
    if path.file_name() != Some(std::ffi::OsStr::new(&update.installer.name)) {
        return Err("本地安装包的文件名与目标版本不符。".to_owned());
    }
    verify_file(path, &update.installer)
}

fn save_installer(body: impl Read, path: &Path, installer: &Installer) -> Result<(), String> {
    let (temporary, mut file) =
        PartialDownload::create(path).map_err(|error| format!("无法保存安装包：{error}"))?;
    let result = write_and_verify(body, &mut file, installer);
    drop(file);
    result?;
    if let Err(error) = fs::rename(&temporary.path, path) {
        // Another process may have installed the same verified download at this path.
        verify_file(path, installer).map_err(|_| format!("无法完成安装包保存：{error}"))?;
    }
    verify_file(path, installer).map(drop)
}

fn write_and_verify(body: impl Read, file: &mut File, installer: &Installer) -> Result<(), String> {
    let bytes = io::copy(&mut body.take(installer.size + 1), file)
        .map_err(|error| format!("安装包下载中断：{error}"))?;
    if bytes != installer.size {
        return Err("下载的安装包大小与 GitHub 记录不符。".to_owned());
    }
    file.sync_all()
        .and_then(|()| file.seek(SeekFrom::Start(0)).map(|_| ()))
        .map_err(|error| format!("无法校验本地安装包：{error}"))?;
    verify_contents(file, installer)
}

fn verify_file(path: &Path, installer: &Installer) -> Result<File, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("无法读取本地安装包：{error}"))?;
    if !metadata.is_file() || metadata.len() != installer.size {
        return Err("本地安装包大小或文件类型不符。".to_owned());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        // Concurrent readers are fine; deny writes and deletion during validation.
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("无法校验本地安装包：{error}"))?;
    verify_contents(&mut file, installer)?;
    Ok(file)
}

fn verify_contents(file: impl Read, installer: &Installer) -> Result<(), String> {
    let mut hash = Sha256::new();
    let count = io::copy(&mut file.take(installer.size + 1), &mut hash)
        .map_err(|error| format!("无法校验本地安装包：{error}"))?;
    let actual: [u8; 32] = hash.finalize().into();
    if count != installer.size || actual != installer.sha256 {
        return Err("安装包 SHA-256 校验失败，请重新下载。".to_owned());
    }
    Ok(())
}

struct PartialDownload {
    path: PathBuf,
}

impl PartialDownload {
    fn create(destination: &Path) -> io::Result<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = destination.with_extension(format!("{pid}-{time}-{sequence}.part"));
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0);
        }
        let file = options.open(&path)?;
        Ok((Self { path }, file))
    }
}

impl Drop for PartialDownload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Clone, Copy)]
enum RequestKind {
    Metadata,
    Installer,
}

struct Response {
    status: u32,
    body: Box<dyn Read>,
}

#[cfg(windows)]
fn get(url: &Url, kind: RequestKind, timeout: Duration) -> io::Result<Response> {
    winhttp::get(url, kind, timeout)
}

#[cfg(not(windows))]
fn get(_url: &Url, _kind: RequestKind, _timeout: Duration) -> io::Result<Response> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "在线更新仅支持 Windows。",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const INSTALLER_BYTES: &[u8] = b"installer test bytes";

    fn release() -> Value {
        json!({
            "tag_name": "v0.10.2",
            "draft": false,
            "prerelease": false,
            "body": "发布说明",
            "assets": [{
                "name": "fishing-0.10.2-windows-x64-setup.exe",
                "state": "uploaded",
                "size": INSTALLER_BYTES.len(),
                "digest": format!("sha256:{:x}", Sha256::digest(INSTALLER_BYTES)),
                "browser_download_url": format!("{RELEASE_DOWNLOADS}v0.10.2/fishing-0.10.2-windows-x64-setup.exe"),
            }],
        })
    }

    fn parse(current: [u32; 3], release: &Value) -> Result<Option<Update>, String> {
        parse_release(current, &serde_json::to_vec(release).unwrap())
    }

    #[test]
    fn compares_versions_numerically_and_only_offers_new_stable_releases() {
        let mut release = release();
        assert_eq!(
            parse([0, 9, 99], &release).unwrap().unwrap().version,
            "0.10.2"
        );
        assert!(parse([0, 10, 2], &release).unwrap().is_none());
        assert!(parse([0, 11, 0], &release).unwrap().is_none());
        release["prerelease"] = json!(true);
        assert!(parse([0, 9, 99], &release).unwrap().is_none());
        for invalid in [
            "01.0.0",
            "1.0",
            "1.0.0.0",
            "1.0.0-beta",
            "1.0.0+build",
            "1.x.0",
            "1.0.+0",
        ] {
            assert!(version(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn requires_exact_installer_name_size_url_and_digest() {
        for (field, value) in [
            ("name", json!("fishing-0.10.2-windows-x64.msi")),
            (
                "browser_download_url",
                json!(
                    "https://github.com/other/fishing/releases/download/v0.10.2/fishing-0.10.2-windows-x64-setup.exe"
                ),
            ),
            ("size", json!(MAX_INSTALLER_BYTES + 1)),
            ("digest", Value::Null),
            ("digest", json!("sha256:invalid")),
            ("state", json!("new")),
        ] {
            let mut payload = release();
            payload["assets"][0][field] = value;
            assert!(parse([0, 9, 99], &payload).is_err(), "{field}");
        }
    }

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn corrupted_or_truncated_payload_never_replaces_a_verified_installer() {
        let update = parse([0, 9, 99], &release()).unwrap().unwrap();
        let scratch = std::env::temp_dir().join(format!(
            "fishing-update-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&scratch).unwrap();
        let directory = TestDirectory(scratch);
        let path = directory.0.join(&update.installer.name);
        save_installer(INSTALLER_BYTES, &path, &update.installer).unwrap();
        let corrupt = vec![0; INSTALLER_BYTES.len()];
        assert!(save_installer(corrupt.as_slice(), &path, &update.installer).is_err());
        assert!(save_installer(&INSTALLER_BYTES[..2], &path, &update.installer).is_err());
        assert_eq!(fs::read(&path).unwrap(), INSTALLER_BYTES);
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
        assert_eq!(download(&update, &directory.0).unwrap(), path);
        let guard = verified_installer(&update, &path).unwrap();
        #[cfg(windows)]
        assert!(fs::write(&path, &corrupt).is_err());
        drop(guard);
        fs::write(&path, corrupt).unwrap();
        assert!(verified_installer(&update, &path).is_err());
    }
}
