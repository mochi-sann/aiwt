//! 軽量なエラー型。外部クレートを増やさず String ベースで扱う。

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
