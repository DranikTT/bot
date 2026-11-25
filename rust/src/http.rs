//! HTTP-клиент поверх системного libcurl.
//!
//! Крейтов нет, поэтому HTTPS берём у libcurl через FFI: он уже стоит
//! в системе почти везде, где живёт бот, и умеет TLS, редиректы и keep-alive.
//! Ссылка на библиотеку — в `build.rs`.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_long, c_void};

#[repr(C)]
struct CurlHandle {
    _private: [u8; 0],
}

extern "C" {
    fn curl_easy_init() -> *mut CurlHandle;
    fn curl_easy_setopt(handle: *mut CurlHandle, option: c_int, ...) -> c_int;
    fn curl_easy_perform(handle: *mut CurlHandle) -> c_int;
    fn curl_easy_cleanup(handle: *mut CurlHandle);
    fn curl_easy_strerror(code: c_int) -> *const c_char;
    fn curl_easy_getinfo(handle: *mut CurlHandle, info: c_int, ...) -> c_int;
    fn curl_slist_append(list: *mut CurlList, value: *const c_char) -> *mut CurlList;
    fn curl_slist_free_all(list: *mut CurlList);
}

#[repr(C)]
struct CurlList {
    _private: [u8; 0],
}

// Опции curl (стабильные значения из curl.h)
const CURLOPT_URL: c_int = 10002;
const CURLOPT_WRITEFUNCTION: c_int = 20011;
const CURLOPT_WRITEDATA: c_int = 10001;
const CURLOPT_TIMEOUT: c_int = 13;
const CURLOPT_CONNECTTIMEOUT: c_int = 78;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_POSTFIELDS: c_int = 10015;
const CURLOPT_POSTFIELDSIZE: c_int = 60;
const CURLOPT_HTTPHEADER: c_int = 10023;
const CURLOPT_USERAGENT: c_int = 10018;
const CURLOPT_NOSIGNAL: c_int = 99;
const CURLOPT_SSL_VERIFYPEER: c_int = 64;
const CURLOPT_SSL_VERIFYHOST: c_int = 81;
const CURLINFO_RESPONSE_CODE: c_int = 0x200002;

/// Ответ сервера.
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Ошибка транспорта (сеть, TLS, таймаут).
#[derive(Debug)]
pub struct HttpError(pub String);

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Колбэк, куда libcurl пишет тело ответа.
extern "C" fn write_callback(
    ptr: *mut c_char,
    size: usize,
    count: usize,
    userdata: *mut c_void,
) -> usize {
    let total = size * count;
    if userdata.is_null() || ptr.is_null() {
        return 0;
    }
    unsafe {
        let buffer = &mut *(userdata as *mut Vec<u8>);
        buffer.extend_from_slice(std::slice::from_raw_parts(ptr as *const u8, total));
    }
    total
}

/// Соединение с Bot API: базовый URL + токен.
pub struct HttpClient {
    base_url: String,
}

impl HttpClient {
    pub fn new(base_url: &str) -> Self {
        // libcurl требует однократной глобальной инициализации
        unsafe {
            curl_easy_init();
        }
        HttpClient {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// POST JSON на произвольный URL.
    pub fn post_json(&self, url: &str, body: &str, timeout_secs: i64) -> Result<Response, HttpError> {
        self.request(url, Some(body), &["Content-Type: application/json; charset=utf-8"], timeout_secs)
    }

    /// POST multipart/form-data — нужен для отправки фото и документов.
    pub fn post_multipart(
        &self,
        url: &str,
        fields: &[(String, String)],
        file_field: Option<(&str, &str, &[u8], &str)>,
        timeout_secs: i64,
    ) -> Result<Response, HttpError> {
        let boundary = format!(
            "----savemod{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );

        let mut body: Vec<u8> = Vec::new();
        for (name, value) in fields {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
            );
            body.extend_from_slice(value.as_bytes());
            body.extend_from_slice(b"\r\n");
        }

        if let Some((field, filename, content, mime)) = file_field {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                    field, filename
                )
                .as_bytes(),
            );
            body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", mime).as_bytes());
            body.extend_from_slice(content);
            body.extend_from_slice(b"\r\n");
        }

        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let content_type = format!("Content-Type: multipart/form-data; boundary={}", boundary);
        self.request(url, Some(&String::from_utf8_lossy(&body)), &[&content_type], timeout_secs)
    }

    /// GET (скачивание файлов по file_path).
    pub fn get(&self, url: &str, timeout_secs: i64) -> Result<Response, HttpError> {
        self.request(url, None, &[], timeout_secs)
    }

    fn request(
        &self,
        url: &str,
        body: Option<&str>,
        headers: &[&str],
        timeout_secs: i64,
    ) -> Result<Response, HttpError> {
        let url_c = CString::new(url).map_err(|_| HttpError("URL содержит NUL".into()))?;
        let mut response_body: Vec<u8> = Vec::new();
        let mut list: *mut CurlList = std::ptr::null_mut();

        unsafe {
            let handle = curl_easy_init();
            if handle.is_null() {
                return Err(HttpError("не удалось инициализировать curl".into()));
            }

            curl_easy_setopt(handle, CURLOPT_URL, url_c.as_ptr());
            curl_easy_setopt(handle, CURLOPT_WRITEFUNCTION, write_callback as *const c_void);
            curl_easy_setopt(
                handle,
                CURLOPT_WRITEDATA,
                &mut response_body as *mut Vec<u8> as *mut c_void,
            );
            curl_easy_setopt(handle, CURLOPT_TIMEOUT, timeout_secs as c_long);
            curl_easy_setopt(handle, CURLOPT_CONNECTTIMEOUT, 20 as c_long);
            curl_easy_setopt(handle, CURLOPT_NOSIGNAL, 1 as c_long);
            curl_easy_setopt(handle, CURLOPT_FOLLOWLOCATION, 1 as c_long);
            curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 1 as c_long);
            curl_easy_setopt(handle, CURLOPT_SSL_VERIFYHOST, 2 as c_long);

            let user_agent = CString::new("savemod-bot/1.0").unwrap();
            curl_easy_setopt(handle, CURLOPT_USERAGENT, user_agent.as_ptr());

            for header in headers {
                if let Ok(value) = CString::new(*header) {
                    list = curl_slist_append(list, value.as_ptr());
                }
            }
            if !list.is_null() {
                curl_easy_setopt(handle, CURLOPT_HTTPHEADER, list);
            }

            if let Some(payload) = body {
                curl_easy_setopt(handle, CURLOPT_POSTFIELDS, payload.as_ptr() as *const c_char);
                curl_easy_setopt(handle, CURLOPT_POSTFIELDSIZE, payload.len() as c_long);
            }

            let code = curl_easy_perform(handle);
            if code != 0 {
                let message = CStr::from_ptr(curl_easy_strerror(code))
                    .to_string_lossy()
                    .into_owned();
                curl_easy_cleanup(handle);
                if !list.is_null() {
                    curl_slist_free_all(list);
                }
                return Err(HttpError(format!("curl {}: {}", code, message)));
            }

            let mut status: c_long = 0;
            curl_easy_getinfo(handle, CURLINFO_RESPONSE_CODE, &mut status as *mut c_long);
            curl_easy_cleanup(handle);
            if !list.is_null() {
                curl_slist_free_all(list);
            }

            Ok(Response {
                status: status as u16,
                body: response_body,
            })
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Кодирует часть пути для URL (кириллица, пробелы и т.п.).
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len() * 3);
    for part in path.split('/') {
        if !out.is_empty() {
            out.push('/');
        }
        for byte in part.as_bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(*byte as char)
                }
                other => out.push_str(&format!("%{:02X}", other)),
            }
        }
    }
    out
}

pub fn mime_of(filename: &str) -> &'static str {
    let lower = filename.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") || lower.ends_with(".oga") {
        "audio/ogg"
    } else if lower.ends_with(".html") {
        "text/html"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn кодирует_путь() {
        assert_eq!(encode_path("documents/file_1.jpg"), "documents/file_1.jpg");
        assert_eq!(encode_path("a b.txt"), "a%20b.txt");
        assert_eq!(encode_path("фото.jpg"), "%D1%84%D0%BE%D1%82%D0%BE.jpg");
    }

    #[test]
    fn определяет_mime() {
        assert_eq!(mime_of("photo.JPG"), "image/jpeg");
        assert_eq!(mime_of("doc.html"), "text/html");
        assert_eq!(mime_of("file.bin"), "application/octet-stream");
    }
}
