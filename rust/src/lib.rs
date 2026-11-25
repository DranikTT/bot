//! Ядро бота: JSON, HTTP, крипто, API Telegram, эмодзи, клавиатуры,
//! хранилище, конфиг, тексты меню, архивы и обработчики.

pub mod api;
pub mod archive;
pub mod config;
pub mod crypto;
pub mod emoji;
pub mod handlers;
pub mod http;
pub mod json;
pub mod keyboard;
pub mod menus;
pub mod sender;
pub mod store;
