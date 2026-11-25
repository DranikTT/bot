//! Линковка с системным libcurl.
//!
//! Крейт `curl-sys` недоступен (crates.io заблокирован), поэтому ищем
//! библиотеку сами: сначала по имени `libcurl.so`/`libcurl.so.4`,
//! затем в типовых каталогах дистрибутивов.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let candidates = [
        "libcurl.so.4",
        "libcurl.so",
        "libcurl-gnutls.so.4",
        "libcurl-gnutls.so.3",
        "libcurl.so.4.8.0",
    ];

    let search_dirs = [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib64",
        "/usr/lib",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib/arm-linux-gnueabihf",
        "/usr/local/lib",
        "/lib/x86_64-linux-gnu",
        "/lib64",
    ];

    // 1. Точное имя файла: в Debian/Ubuntu обычно нет симлинка libcurl.so
    //    (он появляется только с пакетом *-dev), зато есть libcurl.so.4.
    for dir in search_dirs {
        for name in candidates {
            if Path::new(dir).join(name).exists() {
                println!("cargo:rustc-link-search=native={}", dir);
                println!("cargo:rustc-link-arg=-l:{}", name);
                return;
            }
        }
    }

    // 2. Запасной вариант: обычное имя, если симлинк всё-таки есть
    println!("cargo:rustc-link-lib=dylib=curl");

    println!(
        "cargo:warning=libcurl не найден: HTTPS работать не будет. \
         Установите libcurl (apt install libcurl4-openssl-dev или аналог)."
    );
}
