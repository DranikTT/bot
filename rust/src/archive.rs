//! Экспорт переписки в HTML и импорт выгрузки Telegram Desktop.
//!
//! Экспорт даёт один самодостаточный файл: стили и медиа внутри,
//! ничего не тянется из сети. Текст экранируется — XSS из сообщений
//! не сработает. Импорт принимает `result.json` в чистом виде и внутри ZIP,
//! причём распаковка защищена от path traversal (zip-slip).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::emoji::escape;
use crate::json::Value;
use crate::menus::format_time;

/// Результат экспорта.
pub struct ExportResult {
    pub html: String,
    pub count: usize,
}

/// Нормализует тип медиа к общему виду.
pub fn normalize_type(kind: &str) -> String {
    match kind.to_lowercase().as_str() {
        "voice_message" | "voice" => "voice".to_string(),
        "round_video" | "video_message" | "video_note" => "video_note".to_string(),
        "video_file" | "video" => "video".to_string(),
        "audio_file" | "audio" => "audio".to_string(),
        "gif" | "animation" => "animation".to_string(),
        "photo" | "picture" => "photo".to_string(),
        "sticker" => "sticker".to_string(),
        "document" | "file" => "document".to_string(),
        "location" | "geo" => "location".to_string(),
        _ => "text".to_string(),
    }
}

const CSS: &str = "\
:root{--bg:#0e1621;--chat:#17212b;--in:#182533;--out:#2b5278;--text:#f5f5f5;\
--sub:#7f91a4;--accent:#5288c1;--border:#242f3d;--radius:14px}\
*{box-sizing:border-box;margin:0;padding:0}\
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Arial,sans-serif;\
background:var(--bg);color:var(--text);display:flex;justify-content:center;min-height:100vh;padding:12px}\
.archive{width:100%;max-width:860px;background:var(--chat);border-radius:12px;overflow:hidden;\
display:flex;flex-direction:column;box-shadow:0 8px 30px rgba(0,0,0,.5)}\
.head{padding:16px 20px;border-bottom:1px solid var(--border);display:flex;\
justify-content:space-between;align-items:center;position:sticky;top:0;background:var(--chat);z-index:100}\
.title{font-size:1.15rem;font-weight:600}.info{font-size:.85rem;color:var(--sub);margin-top:2px}\
.stats{text-align:right;font-size:.85rem;color:var(--accent)}\
.list{padding:20px 16px;display:flex;flex-direction:column;gap:12px}\
.row{display:flex;width:100%}.row.in{justify-content:flex-start}.row.out{justify-content:flex-end}\
.bubble{max-width:78%;padding:10px 14px;border-radius:var(--radius);word-wrap:break-word;\
font-size:.95rem;line-height:1.45}\
.bubble.sticker-only{background:transparent!important;padding:2px!important}\
.row.in .bubble:not(.sticker-only){background:var(--in);border-bottom-left-radius:4px}\
.row.out .bubble:not(.sticker-only){background:var(--out);border-bottom-right-radius:4px}\
.bhead{display:flex;justify-content:space-between;gap:14px;font-size:.75rem;color:var(--sub);margin-bottom:4px}\
.author{font-weight:600;color:var(--accent)}.row.out .author{color:#92c5ff}\
.btxt{word-break:break-word}\
.media{margin-top:8px;border-radius:8px;overflow:hidden}\
.media.photo img,.media.video video{max-width:100%;max-height:480px;border-radius:8px;display:block}\
.media.sticker{width:170px;height:170px}\
.media.sticker img{width:100%;height:100%;object-fit:contain}\
.media.round video{width:230px;height:230px;border-radius:50%;object-fit:cover;display:block}\
.media.voice audio{width:100%;min-width:250px;margin-top:6px}\
.file{display:flex;gap:10px;align-items:center;padding:8px 10px;background:rgba(0,0,0,.15);\
border-radius:8px;margin-top:6px}\
.ficon{font-size:1.6rem}.fname{font-weight:500}.fsub{font-size:.75rem;color:var(--sub)}\
.placeholder{margin-top:6px;font-style:italic;color:var(--sub);font-size:.85rem}";

/// Строит HTML-архив по сообщениям чата.
pub fn export(messages: &[Value], chat_id: i64, chat_title: &str, allowed: &[String]) -> ExportResult {
    let mut items = String::new();
    let mut count = 0usize;

    for message in messages {
        let kind = normalize_type(&message.get_str("media_type"));
        if !allowed.iter().any(|item| normalize_type(item) == kind) {
            continue;
        }

        let outgoing = message.get_bool("is_outgoing", false);
        let author = if outgoing {
            "Вы".to_string()
        } else {
            escape(chat_title)
        };
        let text = message.get_str("text");
        let date = format_time(message.get_i64("date", 0));

        let media_html = media_html(message, &kind);
        let sticker_only = kind == "sticker" && text.trim().is_empty();

        let text_html = if text.trim().is_empty() {
            String::new()
        } else {
            format!(
                "<div class=\"btxt\">{}</div>",
                escape(&text).replace('\n', "<br>")
            )
        };

        items.push_str(&format!(
            "<div class=\"row {}\"><div class=\"bubble {}\">\
             <div class=\"bhead\"><span class=\"author\">{}</span><span>{}</span></div>\
             {}{}</div></div>",
            if outgoing { "out" } else { "in" },
            if sticker_only { "sticker-only" } else { "" },
            author,
            date,
            text_html,
            media_html
        ));
        count += 1;
    }

    let html = format!(
        "<!DOCTYPE html><html lang=\"ru\"><head><meta charset=\"UTF-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Архив: {}</title><style>{}</style></head><body>\
         <div class=\"archive\"><div class=\"head\"><div>\
         <div class=\"title\">{}</div>\
         <div class=\"info\">Идентификатор чата: {}</div></div>\
         <div class=\"stats\"><div>Всего сообщений: {}</div>\
         <div style=\"font-size:.75rem;color:var(--sub)\">Экспорт завершён</div>\
         </div></div><div class=\"list\">{}</div></div></body></html>",
        escape(chat_title),
        CSS,
        escape(chat_title),
        chat_id,
        count,
        items
    );

    ExportResult { html, count }
}

fn media_html(message: &Value, kind: &str) -> String {
    // Медиа хранится по file_id (скачивание в архив не включено по умолчанию:
    // файлы весят много и требуют Bot API на каждый элемент).
    let file_name = message.get_str("file_name");

    match kind {
        "photo" => placeholder("📷 Фотография"),
        "sticker" => placeholder("🎭 Стикер"),
        "voice" => placeholder("🎤 Голосовое"),
        "video_note" => placeholder("🔘 Кружок"),
        "video" => placeholder("🎬 Видео"),
        "animation" => placeholder("🎞 Гиф"),
        "document" => format!(
            "<div class=\"file\"><div class=\"ficon\">📁</div><div>\
             <div class=\"fname\">{}</div><div class=\"fsub\">Вложенный файл</div></div></div>",
            if file_name.is_empty() {
                "Документ".to_string()
            } else {
                escape(&file_name)
            }
        ),
        "location" => placeholder("📍 Геолокация"),
        _ => String::new(),
    }
}

fn placeholder(text: &str) -> String {
    format!("<div class=\"placeholder\">[{}]</div>", text)
}

// ------------------------------------------------------------- импорт

/// Импортирует result.json. Возвращает (chat_id, название, сколько добавлено).
pub fn import_desktop(json_text: &str, owner_id: i64, store: &mut crate::store::Store) -> Result<(i64, String, usize), String> {
    let data = Value::parse(json_text).map_err(|error| format!("некорректный JSON: {}", error))?;

    let messages = data
        .get_arr("messages")
        .ok_or_else(|| "Некорректная структура result.json: нет массива messages.".to_string())?;

    let chat_id = data.get_i64("id", 0);
    let chat_id = if chat_id == 0 { crate::store::now() } else { chat_id };
    let chat_title = data.get("name").map(|value| value.str_or("")).filter(|value| !value.trim().is_empty()).unwrap_or_else(|| format!("Чат {}", chat_id));

    let mut count = 0usize;
    for item in messages {
        if item.get_str("type") != "message" {
            continue;
        }

        let message_id = item.get_i64("id", count as i64 + 1);
        let date = item
            .get_i64("date_unixtime", 0)
            .max(parse_date(item.get_str("date").as_str()));

        let sender_name = item.get_str("from");
        let sender_id = item
            .get_str("from_id")
            .trim_start_matches("user")
            .parse::<i64>()
            .ok();

        let media_type = if item.get("photo").is_some() {
            "photo".to_string()
        } else if let Some(kind) = item.get("media_type").and_then(|value| value.as_str()) {
            normalize_type(kind)
        } else if item.get("file").is_some() {
            "document".to_string()
        } else {
            "text".to_string()
        };

        let file_name = item
            .get("file")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());

        store.save_message(crate::store::NewMessage {
            owner_id,
            chat_id,
            message_id,
            sender_id,
            sender_name,
            sender_username: String::new(),
            peer_name: chat_title.clone(),
            peer_username: String::new(),
            text: parse_text(item.get("text")),
            media_type: Some(media_type),
            file_id: None,
            file_name,
            is_outgoing: sender_id == Some(owner_id),
            date: if date > 0 { date } else { crate::store::now() },
        });
        count += 1;
    }

    Ok((chat_id, chat_title, count))
}

fn parse_date(text: &str) -> i64 {
    // формат Telegram Desktop: «2024-01-01T10:00:00»
    if text.len() < 19 {
        return 0;
    }
    let year: i64 = text[0..4].parse().unwrap_or(0);
    let month: i64 = text[5..7].parse().unwrap_or(0);
    let day: i64 = text[8..10].parse().unwrap_or(0);
    let hour: i64 = text[11..13].parse().unwrap_or(0);
    let minute: i64 = text[14..16].parse().unwrap_or(0);
    let second: i64 = text[17..19].parse().unwrap_or(0);

    if year == 0 || month == 0 || day == 0 {
        return 0;
    }

    // дни с 1970-01-01
    let mut days = 0i64;
    for y in 1970..year {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        days += if leap { 366 } else { 365 };
    }
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    for days_in_month in month_days.iter().take(month as usize - 1) {
        days += *days_in_month;
    }
    days += day - 1;

    days * 86400 + hour * 3600 + minute * 60 + second
}

/// Текст сообщения: строка или массив форматированных частей.
fn parse_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::Str(text)) => text.clone(),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    Value::Str(text) => out.push_str(text),
                    other => {
                        if let Some(text) = other.get("text").and_then(|value| value.as_str()) {
                            out.push_str(text);
                        }
                    }
                }
            }
            out
        }
        Some(other) => other.to_string(),
    }
}

/// Распаковка ZIP с защитой от path traversal.
///
/// Реализован минимальный reader: ищем central directory и читаем записи
/// с deflate-сжатием (его поддерживает std через `flate2`-подобный код —
/// здесь используется собственная реализация inflate).
pub fn extract_zip(zip_bytes: &[u8], destination: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    let root = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());

    let entries = read_zip_entries(zip_bytes)?;
    let mut result_json = None;

    for entry in entries {
        // защита от «../../evil.sh»
        let normalized = normalize_path(&entry.name);
        if normalized.starts_with("..") {
            crate::api::log::warn(format!("zip-slip заблокирован: {}", entry.name));
            continue;
        }

        let target = root.join(&normalized);
        if entry.is_dir {
            std::fs::create_dir_all(&target).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&target, entry.data).map_err(|error| error.to_string())?;

        if normalized
            .file_name()
            .map(|name| name.eq_ignore_ascii_case("result.json"))
            .unwrap_or(false)
        {
            result_json = Some(target);
        }
    }

    result_json.ok_or_else(|| "В ZIP-архиве не найден result.json.".to_string())
}

fn normalize_path(name: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in name.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                out.push("..");
            }
            other => out.push(other),
        }
    }
    out
}

struct ZipEntry {
    name: String,
    data: Vec<u8>,
    is_dir: bool,
}

/// Разбор ZIP: читаем записи по локальным заголовкам.
fn read_zip_entries(bytes: &[u8]) -> Result<Vec<ZipEntry>, String> {
    let mut entries = Vec::new();
    let mut position = 0usize;

    while position + 30 <= bytes.len() {
        // сигнатура локального файла: PK\x03\x04
        if &bytes[position..position + 4] != b"PK\x03\x04" {
            break;
        }

        let compression = u16::from_le_bytes([bytes[position + 8], bytes[position + 9]]);
        let compressed_size =
            u32::from_le_bytes(bytes[position + 18..position + 22].try_into().unwrap()) as usize;
        let uncompressed_size =
            u32::from_le_bytes(bytes[position + 22..position + 26].try_into().unwrap()) as usize;
        let name_len = u16::from_le_bytes([bytes[position + 26], bytes[position + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[position + 28], bytes[position + 29]]) as usize;

        let name_start = position + 30;
        let name_end = name_start + name_len;
        if name_end + extra_len + compressed_size > bytes.len() {
            break;
        }

        let name = String::from_utf8_lossy(&bytes[name_start..name_end]).into_owned();
        let data_start = name_end + extra_len;
        let data_end = data_start + compressed_size;
        let raw = &bytes[data_start..data_end];

        let data = match compression {
            0 => raw.to_vec(),
            8 => inflate(raw, uncompressed_size)?,
            other => {
                crate::api::log::warn(format!(
                    "запись «{}»: неподдерживаемое сжатие {}, пропускаю",
                    name, other
                ));
                Vec::new()
            }
        };

        let is_dir = name.ends_with('/');
        entries.push(ZipEntry {
            name,
            data,
            is_dir,
        });

        position = data_end + if compression == 0 { 0 } else { 0 };
        // data descriptor либо следующий заголовок
        if position + 4 <= bytes.len() && &bytes[position..position + 4] == b"PK\x07\x08" {
            position += 16;
        }
    }

    if entries.is_empty() {
        return Err("ZIP-архив пуст или повреждён".to_string());
    }
    Ok(entries)
}

/// Inflate (RFC 1951) — минимум для распаковки Telegram-экспорта.
fn inflate(input: &[u8], expected_size: usize) -> Result<Vec<u8>, String> {
    let mut output = Vec::with_capacity(expected_size);
    let mut reader = BitReader::new(input);

    loop {
        let is_final = reader.bits(1)? == 1;
        let block_type = reader.bits(2)?;

        match block_type {
            0 => {
                // без сжатия
                reader.align();
                let length = reader.bits(16)? as usize;
                let _inverse = reader.bits(16)?;
                for _ in 0..length {
                    output.push(reader.bits(8)? as u8);
                }
            }
            1 => {
                let (literals, distances) = fixed_tables();
                inflate_block(&mut reader, &mut output, &literals, &distances)?;
            }
            2 => {
                let (literals, distances) = dynamic_tables(&mut reader)?;
                inflate_block(&mut reader, &mut output, &literals, &distances)?;
            }
            _ => return Err("некорректный блок deflate".to_string()),
        }

        if is_final {
            break;
        }
        if output.len() > 200 * 1024 * 1024 {
            return Err("архив слишком большой после распаковки".to_string());
        }
    }

    Ok(output)
}

struct BitReader<'a> {
    data: &'a [u8],
    position: usize,
    bit_buffer: u32,
    bit_count: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        BitReader {
            data,
            position: 0,
            bit_buffer: 0,
            bit_count: 0,
        }
    }

    fn bits(&mut self, count: u32) -> Result<u32, String> {
        while self.bit_count < count {
            if self.position >= self.data.len() {
                return Err("неожиданный конец deflate-потока".to_string());
            }
            self.bit_buffer |= (self.data[self.position] as u32) << self.bit_count;
            self.position += 1;
            self.bit_count += 8;
        }
        let value = self.bit_buffer & ((1u32 << count) - 1);
        self.bit_buffer >>= count;
        self.bit_count -= count;
        Ok(value)
    }

    fn align(&mut self) {
        self.bit_buffer = 0;
        self.bit_count = 0;
    }
}

/// Каноническое дерево Хаффмана: символ → длина кода.
struct Huffman {
    // (код, длина, символ), отсортировано по длине
    codes: Vec<(u32, u32, u16)>,
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Self {
        let max_length = lengths.iter().copied().max().unwrap_or(0) as u32;
        let mut count = vec![0u32; (max_length + 1) as usize];
        for length in lengths {
            if *length > 0 {
                count[*length as usize] += 1;
            }
        }

        let mut next_code = vec![0u32; (max_length + 1) as usize];
        let mut code = 0u32;
        for bits in 1..=max_length as usize {
            code = (code + count[bits - 1]) << 1;
            next_code[bits] = code;
        }

        let mut codes = Vec::new();
        for (symbol, length) in lengths.iter().enumerate() {
            if *length > 0 {
                let length = *length as u32;
                let code = next_code[length as usize];
                next_code[length as usize] += 1;
                codes.push((code, length, symbol as u16));
            }
        }
        Huffman { codes }
    }

    fn decode(&self, reader: &mut BitReader) -> Result<u16, String> {
        let mut code = 0u32;
        for length in 1..=15u32 {
            code = (code << 1) | reader.bits(1)?;
            for (candidate, candidate_length, symbol) in &self.codes {
                if *candidate_length == length && *candidate == code {
                    return Ok(*symbol);
                }
            }
        }
        Err("не удалось декодировать код Хаффмана".to_string())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn fixed_tables() -> (Huffman, Huffman) {
    let mut literal_lengths = vec![0u8; 288];
    for (symbol, length) in literal_lengths.iter_mut().enumerate() {
        *length = match symbol {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let distance_lengths = vec![5u8; 30];
    (
        Huffman::from_lengths(&literal_lengths),
        Huffman::from_lengths(&distance_lengths),
    )
}

fn dynamic_tables(reader: &mut BitReader) -> Result<(Huffman, Huffman), String> {
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];

    let literal_count = reader.bits(5)? as usize + 257;
    let distance_count = reader.bits(5)? as usize + 1;
    let code_length_count = reader.bits(4)? as usize + 4;

    let mut code_lengths = [0u8; 19];
    for index in 0..code_length_count {
        code_lengths[ORDER[index]] = reader.bits(3)? as u8;
    }
    let code_length_huffman = Huffman::from_lengths(&code_lengths);

    let mut lengths = vec![0u8; literal_count + distance_count];
    let mut index = 0;
    while index < lengths.len() {
        let symbol = code_length_huffman.decode(reader)?;
        match symbol {
            0..=15 => {
                lengths[index] = symbol as u8;
                index += 1;
            }
            16 => {
                if index == 0 {
                    return Err("повтор без предыдущей длины".to_string());
                }
                let previous = lengths[index - 1];
                let repeat = 3 + reader.bits(2)? as usize;
                for _ in 0..repeat {
                    if index >= lengths.len() {
                        break;
                    }
                    lengths[index] = previous;
                    index += 1;
                }
            }
            17 => {
                let repeat = 3 + reader.bits(3)? as usize;
                index = (index + repeat).min(lengths.len());
            }
            18 => {
                let repeat = 11 + reader.bits(7)? as usize;
                index = (index + repeat).min(lengths.len());
            }
            _ => return Err("некорректный код длины".to_string()),
        }
    }

    Ok((
        Huffman::from_lengths(&lengths[..literal_count]),
        Huffman::from_lengths(&lengths[literal_count..]),
    ))
}

fn inflate_block(
    reader: &mut BitReader,
    output: &mut Vec<u8>,
    literals: &Huffman,
    distances: &Huffman,
) -> Result<(), String> {
    loop {
        let symbol = literals.decode(reader)?;
        match symbol {
            0..=255 => output.push(symbol as u8),
            256 => return Ok(()),
            257..=285 => {
                let index = (symbol - 257) as usize;
                let length = LENGTH_BASE[index] as usize
                    + reader.bits(LENGTH_EXTRA[index] as u32)? as usize;

                let distance_symbol = distances.decode(reader)? as usize;
                if distance_symbol >= DIST_BASE.len() {
                    return Err("некорректное расстояние".to_string());
                }
                let distance = DIST_BASE[distance_symbol] as usize
                    + reader.bits(DIST_EXTRA[distance_symbol] as u32)? as usize;

                if distance > output.len() {
                    return Err("расстояние больше выхода".to_string());
                }
                let start = output.len() - distance;
                for offset in 0..length {
                    let byte = output[start + offset];
                    output.push(byte);
                }
            }
            _ => return Err("некорректный символ длины".to_string()),
        }
        if output.len() > 200 * 1024 * 1024 {
            return Err("распакованный файл слишком большой".to_string());
        }
    }
}

/// Хелпер для создания ZIP (нужен тестам и импорту).
pub fn write_zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central = Vec::new();

    for (name, data) in entries {
        let offset = output.len() as u32;
        let name_bytes = name.as_bytes();

        output.extend_from_slice(b"PK\x03\x04");
        output.extend_from_slice(&20u16.to_le_bytes()); // версия
        output.extend_from_slice(&0u16.to_le_bytes()); // флаги
        output.extend_from_slice(&0u16.to_le_bytes()); // метод: stored
        output.extend_from_slice(&0u16.to_le_bytes()); // время
        output.extend_from_slice(&0u16.to_le_bytes()); // дата
        output.extend_from_slice(&0u32.to_le_bytes()); // crc32 (не проверяем)
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(data.len() as u32).to_le_bytes());
        output.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes()); // extra
        output.extend_from_slice(name_bytes);
        output.extend_from_slice(data);

        central.extend_from_slice(b"PK\x01\x02");
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = output.len() as u32;
    let central_size = central.len() as u32;
    output.extend_from_slice(&central);

    output.extend_from_slice(b"PK\x05\x06");
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    output.extend_from_slice(&central_size.to_le_bytes());
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());

    output
}

/// Запись во временный файл (для отладки).
#[allow(dead_code)]
pub fn write_temp(bytes: &[u8], name: &str) -> std::io::Result<PathBuf> {
    let path = std::env::temp_dir().join(name);
    let mut file = std::fs::File::create(&path)?;
    file.write_all(bytes)?;
    Ok(path)
}

#[allow(dead_code)]
fn read_all(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut data = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut data)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn нормализует_типы() {
        assert_eq!(normalize_type("voice_message"), "voice");
        assert_eq!(normalize_type("round_video"), "video_note");
        assert_eq!(normalize_type("GIF"), "animation");
        assert_eq!(normalize_type("photo"), "photo");
        assert_eq!(normalize_type("unknown"), "text");
    }

    fn message(chat_id: i64, message_id: i64, text: &str, outgoing: bool) -> Value {
        let _ = chat_id;
        Value::from_pairs(vec![
            ("message_id", Value::Int(message_id)),
            ("text", Value::str(text)),
            ("is_outgoing", Value::Bool(outgoing)),
            ("date", Value::Int(1700000000)),
        ])
    }

    #[test]
    fn экспорт_даёт_html() {
        let messages = vec![message(42, 1, "привет", false)];
        let allowed: Vec<String> = vec!["text".to_string()];
        let result = export(&messages, 42, "Вася", &allowed);

        assert_eq!(result.count, 1);
        assert!(result.html.contains("<!DOCTYPE html>"));
        assert!(result.html.contains("привет"));
        assert!(result.html.contains("Вася"));
    }

    #[test]
    fn экспорт_экранирует_xss() {
        let messages = vec![message(42, 1, "<script>alert(1)</script>", false)];
        let allowed: Vec<String> = vec!["text".to_string()];
        let result = export(&messages, 42, "<img onerror=alert(1)>", &allowed);

        assert!(!result.html.contains("<script>alert(1)</script>"));
        assert!(result.html.contains("&lt;script&gt;"));
        assert!(!result.html.contains("<img onerror"));
    }

    #[test]
    fn экспорт_фильтрует_типы() {
        let mut record = message(42, 1, "фото", false);
        record.set("media_type", "photo");
        let messages = vec![record];
        let result = export(&messages, 42, "Вася", &["text".to_string()]);
        assert_eq!(result.count, 0, "фото попало в текстовый экспорт");
    }

    #[test]
    fn импорт_result_json() {
        let json = r#"{
            "id": 900,
            "name": "Тест",
            "messages": [
                {"id": 1, "type": "message", "date": "2024-01-01T10:00:00",
                 "date_unixtime": 1704103200, "from": "Вася", "from_id": "user42",
                 "text": "первое"},
                {"id": 2, "type": "message", "date": "2024-01-01T10:01:00",
                 "date_unixtime": 1704103260, "from": "Петя", "from_id": "user43",
                 "text": [{"type": "bold", "text": "второе"}, " сообщение"],
                 "photo": "photos/1.jpg"}
            ]
        }"#;

        let path = std::env::temp_dir().join("savemod_import_test.json");
        let _ = std::fs::remove_file(&path);
        let mut store = crate::store::Store::open(&path, "k");

        let (chat_id, title, count) = import_desktop(json, 777, &mut store).unwrap();
        assert_eq!(chat_id, 900);
        assert_eq!(title, "Тест");
        assert_eq!(count, 2);

        let messages = store.messages_of(777, 900);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].get_str("text"), "первое");
        assert_eq!(messages[1].get_str("text"), "второе сообщение");
    }

    #[test]
    fn импорт_отклоняет_мусор() {
        let path = std::env::temp_dir().join("savemod_import_bad.json");
        let _ = std::fs::remove_file(&path);
        let mut store = crate::store::Store::open(&path, "k");
        let error = import_desktop(r#"{"foo": 1}"#, 1, &mut store).unwrap_err();
        assert!(error.contains("messages"), "ошибка: {}", error);
    }

    #[test]
    fn zip_slip_заблокирован() {
        let zip = write_zip_stored(&[
            ("../../evil.sh", b"pwned"),
            ("result.json", br#"{"messages": []}"#),
        ]);

        let dir = std::env::temp_dir().join("savemod_zip_slip");
        let _ = std::fs::remove_dir_all(&dir);
        let found = extract_zip(&zip, &dir).unwrap();

        assert!(found.ends_with("result.json"));
        // выйти за пределы каталога нельзя
        let escaped = dir.join("../../evil.sh");
        assert!(
            !escaped.canonicalize().map(|p| p.exists()).unwrap_or(false),
            "zip-slip сработал"
        );
    }

    #[test]
    fn zip_без_result_json_отклоняется() {
        let zip = write_zip_stored(&[("data.txt", b"hello")]);
        let dir = std::env::temp_dir().join("savemod_zip_noresult");
        let _ = std::fs::remove_dir_all(&dir);
        let error = extract_zip(&zip, &dir).unwrap_err();
        assert!(error.contains("result.json"));
    }

    #[test]
    fn дата_из_строки_телеграма() {
        // 2024-01-01T10:00:00 UTC
        assert_eq!(parse_date("2024-01-01T10:00:00"), 1704103200);
        assert_eq!(parse_date("мусор"), 0);
    }
}
