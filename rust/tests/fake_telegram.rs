//! Фейковый сервер Telegram Bot API для интеграционных тестов.
//!
//! Полная замена живому Telegram: принимает запросы, записывает вызовы
//! и умеет имитировать боевые отказы — DOCUMENT_INVALID на кастомные эмодзи
//! и «there is no text in the message to edit». Именно на этих отказах
//! раньше падал прод, поэтому проверяем фолбэк по-настоящему.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use savemod_bot::json::Value;

/// Один записанный вызов.
#[derive(Clone, Debug)]
pub struct Call {
    pub method: String,
    pub payload: Value,
}

impl Call {
    pub fn text(&self) -> String {
        self.payload.get_str("text")
    }

    pub fn caption(&self) -> String {
        self.payload.get_str("caption")
    }

    /// Текст или подпись — что найдётся.
    pub fn any_text(&self) -> String {
        let text = self.text();
        if text.is_empty() {
            self.caption()
        } else {
            text
        }
    }

    pub fn has_emoji_tags(&self) -> bool {
        self.any_text().contains("<tg-emoji")
    }

    pub fn keyboard(&self) -> Option<Value> {
        self.payload.get("reply_markup").cloned()
    }
}

#[derive(Default)]
struct State {
    port: u16,
    queue: Vec<Value>,
    calls: Vec<Call>,
    errors: Vec<(String, String)>,
    reject_custom_emoji: bool,
    no_text_messages: Vec<i64>,
}

pub struct FakeTelegram {
    state: Arc<Mutex<State>>,
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl FakeTelegram {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("не удалось занять порт");
        let port = listener.local_addr().unwrap().port();
        listener
            .set_nonblocking(true)
            .expect("не удалось включить nonblocking");

        let state = Arc::new(Mutex::new(State {
            port,
            ..Default::default()
        }));
        let running = Arc::new(AtomicBool::new(true));

        let thread_state = Arc::clone(&state);
        let thread_running = Arc::clone(&running);
        let handle = thread::spawn(move || {
            while thread_running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = handle_connection(stream, &thread_state);
                    }
                    Err(ref error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        FakeTelegram {
            state,
            handle: Some(handle),
            running,
        }
    }

    pub fn base_url(&self) -> String {
        let port = self.state.lock().unwrap().port;
        format!("http://127.0.0.1:{}", port)
    }

    /// Кладёт обновление в очередь getUpdates (используется сквозными тестами).
    #[allow(dead_code)]
    pub fn push_update(&self, update: Value) {
        self.state.lock().unwrap().queue.push(update);
    }

    pub fn calls_of(&self, method: &str) -> Vec<Call> {
        self.state
            .lock()
            .unwrap()
            .calls
            .iter()
            .filter(|call| call.method == method)
            .cloned()
            .collect()
    }

    pub fn all_calls(&self) -> Vec<Call> {
        self.state.lock().unwrap().calls.clone()
    }

    pub fn errors(&self) -> Vec<(String, String)> {
        self.state.lock().unwrap().errors.clone()
    }

    #[allow(dead_code)]
    pub fn last_call(&self, method: &str) -> Option<Call> {
        self.calls_of(method).last().cloned()
    }

    /// Включает отказ на кастомные эмодзи (как в проде: DOCUMENT_INVALID).
    pub fn reject_custom_emoji(&self, value: bool) {
        self.state.lock().unwrap().reject_custom_emoji = value;
    }

    /// Помечает сообщение как нередактируемое (фото без подписи).
    pub fn mark_no_text(&self, message_id: i64) {
        self.state.lock().unwrap().no_text_messages.push(message_id);
    }

    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.calls.clear();
        state.errors.clear();
        state.queue.clear();
    }

    /// Тексты всех вызовов подряд — удобно для отладки в assert-сообщении.
    pub fn timeline(&self) -> Vec<String> {
        self.all_calls()
            .iter()
            .map(|call| format!("{}: {}", call.method, call.any_text()))
            .collect()
    }
}

impl Drop for FakeTelegram {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(mut stream: TcpStream, state: &Arc<Mutex<State>>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    // заголовки
    let mut headers = Vec::new();
    let mut buffer = [0u8; 1];
    loop {
        if stream.read(&mut buffer)? == 0 {
            return Ok(());
        }
        headers.push(buffer[0]);
        if headers.ends_with(b"\r\n\r\n") {
            break;
        }
        if headers.len() > 64 * 1024 {
            return Ok(());
        }
    }

    let header_text = String::from_utf8_lossy(&headers).to_string();
    let mut content_length = 0usize;
    for line in header_text.lines() {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body)?;
    }

    let request_line = header_text.lines().next().unwrap_or("").to_string();
    let path = request_line.split_whitespace().nth(1).unwrap_or("").to_string();

    // /bot<token>/<method> или /file/bot<token>/<path>
    let method = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();

    let body_text = String::from_utf8_lossy(&body).to_string();
    let payload = if header_text.to_lowercase().contains("multipart/form-data") {
        parse_multipart(&header_text, &body_text)
    } else if body_text.trim().is_empty() {
        Value::obj()
    } else {
        Value::parse(&body_text).unwrap_or_else(|_| Value::obj())
    };

    let mut state = state.lock().unwrap();

    let response = if method == "getUpdates" {
        let updates: Vec<Value> = state.queue.drain(..).collect();
        ok_value(Value::Array(updates))
    } else if method == "getMe" {
        ok_value(Value::from_pairs(vec![
            ("id", Value::Int(42)),
            ("is_bot", Value::Bool(true)),
            ("username", Value::str("test_bot")),
            ("first_name", Value::str("Test")),
        ]))
    } else {
        state.calls.push(Call {
            method: method.clone(),
            payload: payload.clone(),
        });

        let text = payload.get_str("text");
        let caption = payload.get_str("caption");
        let has_emoji = text.contains("<tg-emoji") || caption.contains("<tg-emoji");
        let message_id = payload.get_i64("message_id", 0);

        if state.reject_custom_emoji && has_emoji {
            state.errors.push((method.clone(), "DOCUMENT_INVALID".to_string()));
            error_value(
                400,
                "Bad Request: BUTTON_ICON_INVALID или DOCUMENT_INVALID: кастомные эмодзи недоступны",
            )
        } else if method == "editMessageText" && state.no_text_messages.contains(&message_id) {
            state.errors.push((
                method.clone(),
                "no text in the message to edit".to_string(),
            ));
            error_value(400, "Bad Request: there is no text in the message to edit")
        } else {
            ok_value(Value::from_pairs(vec![
                ("message_id", Value::Int(if message_id > 0 { message_id } else { 1001 })),
                (
                    "chat",
                    Value::from_pairs(vec![("id", Value::Int(payload.get_i64("chat_id", 0)))]),
                ),
            ]))
        }
    };

    let body = response.to_string();
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Минимальный разбор multipart: только текстовые поля.
fn parse_multipart(headers: &str, body: &str) -> Value {
    let boundary = headers
        .lines()
        .find(|line| line.to_lowercase().contains("boundary="))
        .and_then(|line| line.split("boundary=").nth(1))
        .map(|value| value.trim().trim_matches('"').to_string())
        .unwrap_or_default();

    let mut payload = BTreeMap::new();
    for part in body.split(&format!("--{}", boundary)) {
        let Some((part_headers, value)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let Some(name) = part_headers
            .lines()
            .find(|line| line.contains("name=\""))
            .and_then(|line| line.split("name=\"").nth(1))
            .and_then(|value| value.split('"').next())
        else {
            continue;
        };
        // файл: запоминаем имя, тело не разбираем
        if let Some(filename) = part_headers
            .lines()
            .find(|line| line.contains("filename=\""))
            .and_then(|line| line.split("filename=\"").nth(1))
            .and_then(|value| value.split('"').next())
        {
            payload.insert(name.to_string(), Value::str(filename));
            continue;
        }

        let value = value.trim_end_matches("\r\n").trim_end_matches("--").trim();
        if value.is_empty() {
            continue;
        }
        payload.insert(name.to_string(), Value::str(value));
    }
    Value::Object(payload)
}

fn ok_value(result: Value) -> Value {
    Value::from_pairs(vec![
        ("ok", Value::Bool(true)),
        ("result", result),
    ])
}

fn error_value(code: i64, description: &str) -> Value {
    Value::from_pairs(vec![
        ("ok", Value::Bool(false)),
        ("error_code", Value::Int(code)),
        ("description", Value::str(description)),
    ])
}
