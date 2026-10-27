//! (モデル × エフォート)の乗り換え先を、根拠付きで判定する。
//!
//! - `stats`: 二項割合の差の区間(Newcombe 1998)。
//! - `judge`: 同等性検定(TOST、Lakens 2017)と、役ごとの束ね方。
//! - `map`: 近さの地図(L∞ の距離と、古典的 MDS による 2 次元への射影、stress)。
//! - `lens`: 過去の課題の内訳(種類ごとの件数)。どのベンチを見るかを寄せるのに使う。
//! - `render`: 単一の HTML ページ(データとスクリプトを埋め込む)の生成。
//!
//! 入力のカタログは `model-catalog` の `catalog.toml` が正本で、ここは判定の規則だけを持つ。

pub mod judge;
pub mod lens;
pub mod map;
pub mod render;
pub mod stats;
