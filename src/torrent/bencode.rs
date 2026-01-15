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

    /// 从 bencode 数据解码
    pub fn decode(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() {
            return Err("Empty data".to_string());
        }

        let first = data[0];
        match first {
            b'd' => Self::decode_dict(data),
            b'l' => Self::decode_list(data),
            b'i' => Self::decode_int(data),
            b'0'..=b'9' => Self::decode_bytes(data),
            _ => Err(format!("Invalid bencode starting character: {}", first)),
        }
    }

    /// 解码字典
    fn decode_dict(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() || data[0] != b'd' {
            return Err("Invalid dict".to_string());
        }

        let mut dict = BTreeMap::new();
        let mut pos = 1;

        while pos < data.len() && data[pos] != b'e' {
            // 解码键 (必须是字节字符串)
            let (key_value, key_len) = Self::decode_bytes(&data[pos..])?;
            pos += key_len;

            // 将键转换为 Vec<u8>
            let key_bytes = match key_value {
                BencodeValue::String(s) => s.into_bytes(),
                BencodeValue::Bytes(b) => b,
                _ => return Err("Dict key must be string or bytes".to_string()),
            };

            // 解码值
            let (value, value_len) = Self::decode(&data[pos..])?;
            pos += value_len;

            dict.insert(key_bytes, value);
        }

        if pos >= data.len() {
            return Err("Unterminated dict".to_string());
        }

        Ok((BencodeValue::Dict(dict), pos + 1)) // +1 for 'e'
    }

    /// 解码列表
    fn decode_list(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() || data[0] != b'l' {
            return Err("Invalid list".to_string());
        }

        let mut list = Vec::new();
        let mut pos = 1;

        while pos < data.len() && data[pos] != b'e' {
            let (value, len) = Self::decode(&data[pos..])?;
            list.push(value);
            pos += len;
        }

        if pos >= data.len() {
            return Err("Unterminated list".to_string());
        }

        Ok((BencodeValue::List(list), pos + 1)) // +1 for 'e'
    }

    /// 解码整数
    fn decode_int(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        if data.is_empty() || data[0] != b'i' {
            return Err("Invalid int".to_string());
        }

        let end = data[1..].iter().position(|&b| b == b'e')
            .ok_or_else(|| "Unterminated int".to_string())? + 1;

        let num_str = std::str::from_utf8(&data[1..=end])
            .map_err(|_| "Invalid UTF-8 in int".to_string())?;

        let value = num_str.parse::<i64>()
            .map_err(|_| "Invalid integer".to_string())?;

        Ok((BencodeValue::Int(value), end + 2)) // +2 for 'i' and 'e'
    }

    /// 解码字节字符串
    fn decode_bytes(data: &[u8]) -> Result<(BencodeValue, usize), String> {
        // 查找冒号
        let colon_pos = data.iter().position(|&b| b == b':')
            .ok_or_else(|| "Missing colon in bytes string".to_string())?;

        let len_str = std::str::from_utf8(&data[..colon_pos])
            .map_err(|_| "Invalid UTF-8 in length".to_string())?;

        let len = len_str.parse::<usize>()
            .map_err(|_| "Invalid length".to_string())?;

        let start = colon_pos + 1;
        let end = start + len;

        if data.len() < end {
            return Err("Unexpected end of data".to_string());
        }

        let bytes = data[start..end].to_vec();

        // 尝试解析为 UTF-8 字符串
        if let Ok(s) = String::from_utf8(bytes.clone()) {
            Ok((BencodeValue::String(s), end))
        } else {
            Ok((BencodeValue::Bytes(bytes), end))
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
