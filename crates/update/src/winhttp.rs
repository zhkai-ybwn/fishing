use crate::{RequestKind, Response};
use std::ffi::c_void;
use std::io::{self, Read};
use std::ptr;
use std::time::{Duration, Instant};
use url::Url;
use windows_sys::{
    Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_DISABLE_AUTHENTICATION,
        WINHTTP_DISABLE_COOKIES, WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_ESCAPE_DISABLE,
        WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_QUERY_FLAG_NUMBER,
        WINHTTP_QUERY_LOCATION, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    },
    w,
};

const MAX_REDIRECTS: usize = 3;
const MAX_OPERATION_TIMEOUT: Duration = Duration::from_secs(12);

pub(super) fn get(url: &Url, kind: RequestKind, timeout: Duration) -> io::Result<Response> {
    let deadline = Instant::now() + timeout;
    let mut url = url.clone();
    let expected_host = match kind {
        RequestKind::Metadata => "api.github.com",
        RequestKind::Installer => "github.com",
    };
    if !is_https(&url) || url.host_str() != Some(expected_host) {
        return Err(invalid_response("更新请求的站点无效。"));
    }
    for redirects in 0..=MAX_REDIRECTS {
        let body = request(&url, kind, deadline)?;
        let status = body.status()?;
        if !matches!(status, 301 | 302 | 303 | 307 | 308) {
            return Ok(Response {
                status,
                body: Box::new(body),
            });
        }
        if matches!(kind, RequestKind::Metadata) || redirects == MAX_REDIRECTS {
            return Err(invalid_response("GitHub 返回了意外的更新下载跳转。"));
        }
        url = redirect_url(&url, &body.location()?)?;
    }
    Err(invalid_response("安装包下载跳转次数过多。"))
}

fn is_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

fn redirect_url(current: &Url, location: &str) -> io::Result<Url> {
    let url = current
        .join(location)
        .map_err(|_| invalid_response("安装包下载跳转地址无效。"))?;
    if !is_https(&url)
        || !matches!(
            url.host_str(),
            Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com")
        )
    {
        return Err(invalid_response("安装包下载跳转到了不受信任的站点。"));
    }
    Ok(url)
}

fn request(url: &Url, kind: RequestKind, deadline: Instant) -> io::Result<HttpBody> {
    let timeout = remaining(deadline)?
        .min(MAX_OPERATION_TIMEOUT)
        .as_millis()
        .max(1) as i32;
    let host = wide(
        url.host_str()
            .ok_or_else(|| invalid_response("更新请求缺少站点名称。"))?,
    );
    // The URL parser leaves the path and query in the escaped form expected by WinHTTP.
    let resource = wide(&url[url::Position::BeforePath..url::Position::AfterQuery]);
    let headers = match kind {
        RequestKind::Metadata => {
            w!("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2026-03-10\r\n")
        }
        RequestKind::Installer => ptr::null(),
    };
    // SAFETY: Each string is terminated and each handle is checked at creation, owned, and closed.
    // HTTPS certificate validation is performed by WinHTTP with the OS trust configuration.
    unsafe {
        let session = Handle::new(WinHttpOpen(
            w!("fishing updater"),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ))?;
        check(WinHttpSetTimeouts(
            session.0,
            timeout.min(5000),
            timeout,
            timeout,
            timeout,
        ))?;
        let connection = Handle::new(WinHttpConnect(session.0, host.as_ptr(), 443, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            resource.as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE | WINHTTP_FLAG_ESCAPE_DISABLE,
        ))?;
        // Follow only the redirects validated in get(), and avoid sending account credentials or cookies.
        let disabled =
            WINHTTP_DISABLE_REDIRECTS | WINHTTP_DISABLE_COOKIES | WINHTTP_DISABLE_AUTHENTICATION;
        check(WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_DISABLE_FEATURE,
            ptr::from_ref(&disabled).cast(),
            std::mem::size_of_val(&disabled) as u32,
        ))?;
        let header_length = if headers.is_null() { 0 } else { u32::MAX };
        check(WinHttpSendRequest(
            request.0,
            headers,
            header_length,
            ptr::null(),
            0,
            0,
            0,
        ))?;
        remaining(deadline)?;
        check(WinHttpReceiveResponse(request.0, ptr::null_mut()))?;
        remaining(deadline)?;
        Ok(HttpBody {
            request,
            _connection: connection,
            _session: session,
            deadline,
        })
    }
}

struct Handle(*mut c_void);

impl Handle {
    fn new(handle: *mut c_void) -> io::Result<Self> {
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: Only successful calls produce a Handle, and this is its sole owner.
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}

struct HttpBody {
    // Fields drop in declaration order. Close the request before its connection and session.
    request: Handle,
    _connection: Handle,
    _session: Handle,
    deadline: Instant,
}

impl HttpBody {
    fn status(&self) -> io::Result<u32> {
        let mut status = 0u32;
        let mut bytes = std::mem::size_of_val(&status) as u32;
        // SAFETY: WinHTTP writes the numeric status into this writable u32.
        check(unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                ptr::null(),
                ptr::from_mut(&mut status).cast(),
                &mut bytes,
                ptr::null_mut(),
            )
        })?;
        Ok(status)
    }

    fn location(&self) -> io::Result<String> {
        let mut buffer = [0u16; 8192];
        let mut bytes = std::mem::size_of_val(&buffer) as u32;
        // SAFETY: WinHTTP receives the writable UTF-16 buffer and its capacity in bytes.
        check(unsafe {
            WinHttpQueryHeaders(
                self.request.0,
                WINHTTP_QUERY_LOCATION,
                ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
                ptr::null_mut(),
            )
        })?;
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end]).map_err(|_| invalid_response("安装包下载跳转地址无效。"))
    }
}

impl Read for HttpBody {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        remaining(self.deadline)?;
        let size = buffer.len().min(u32::MAX as usize) as u32;
        let mut bytes = 0;
        // SAFETY: The output pointer and size come from the same mutable slice.
        check(unsafe {
            WinHttpReadData(self.request.0, buffer.as_mut_ptr().cast(), size, &mut bytes)
        })?;
        remaining(self.deadline)?;
        Ok(bytes as usize)
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "连接 GitHub 超时。"))
}

fn check(success: i32) -> io::Result<()> {
    if success != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn invalid_response(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_are_limited_to_https_github_asset_hosts() {
        let source =
            Url::parse("https://github.com/zhkai-ybwn/fishing/releases/download/v0.2.0/file.exe")
                .unwrap();
        for url in [
            "https://release-assets.githubusercontent.com/github-production-release-asset-2e65be/object?sig=test",
            "https://objects.githubusercontent.com/asset",
        ] {
            assert!(redirect_url(&source, url).is_ok());
        }
        for url in [
            "http://release-assets.githubusercontent.com/asset",
            "https://release-assets.githubusercontent.com.example.net/asset",
            "https://user@objects.githubusercontent.com/asset",
            "https://objects.githubusercontent.com:8443/asset",
            "https://objects.githubusercontent.com/asset#fragment",
            "https://127.0.0.1/asset",
        ] {
            assert!(redirect_url(&source, url).is_err());
        }
    }
}
