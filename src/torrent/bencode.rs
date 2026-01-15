//! 简单的 bencode 编码器实现
//!
//! 用于 .torrent 文件的编码和解码

use std::collections::BTreeMap;

/// Bencode 值
#[derive(Debug, Clone)]
pub enum BencodeValue {
    String(String),
    Bytes(Vec<u8>),
    Int(i64),
    List(Vec<BencodeValue>),
    Dict(BTreeMap<Vec<u8>, BencodeValue>),
}

impl BencodeValue {
    /// 编码为 bencode 格式
    pub fn encode(&self) -> Vec<u8> {
        match self {
            BencodeValue::String(s) => {
                format!("{}:{}{}", s.len(), s, "").into_bytes()
            }
            BencodeValue::Bytes(b) => {
                let mut result = format!("{}:", b.len()).into_bytes();
                result.extend_from_slice(b);
                result
            }
            BencodeValue::Int(n) => {
                format!("i{}e", n).into_bytes()
            }
            BencodeValue::List(list) => {
                let mut result = b"l".to_vec();
                for item in list {
                    result.extend_from_slice(&item.encode());
                }
                result.push(b'e');
                result
            }
            BencodeValue::Dict(dict) => {
                let mut result = b"d".to_vec();
                // BTreeMap 确保键按字典序排列（bencode 规范要求）
                for (key, value) in dict {
                    result.extend_from_slice(&encode_bytes(&key));
                    result.extend_from_slice(&value.encode());
                }
                result.push(b'e');
                result
            }
        }
    }
}

/// 编码字节字符串
fn encode_bytes(bytes: &[u8]) -> Vec<u8> {
    format!("{}:", bytes.len()).into_bytes()
        .into_iter()
        .chain(bytes.iter().copied())
        .collect()
}

/// 编码字符串
pub fn encode_string(s: &str) -> Vec<u8> {
    BencodeValue::String(s.to_string()).encode()
}

/// 编码整数
pub fn encode_int(n: i64) -> Vec<u8> {
    BencodeValue::Int(n).encode()
}

/// 编码字典
pub fn encode_dict(dict: &BTreeMap<Vec<u8>, BencodeValue>) -> Vec<u8> {
    BencodeValue::Dict(dict.clone()).encode()
}

/// 编码列表
pub fn encode_list(list: &[BencodeValue]) -> Vec<u8> {
    BencodeValue::List(list.to_vec()).encode()
}
