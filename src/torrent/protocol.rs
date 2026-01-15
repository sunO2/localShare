//! BitWire 协议实现
//!
//! BitTorrent 对等连接协议，包括握手和消息格式

use crate::common::error::{Error, Result};
use std::io::{Cursor, Read};

/// 协议握手字符串
pub const PROTOCOL_STRING: &[u8] = b"BitTorrent protocol";

/// 握手消息长度 (49 + 19 = 68)
pub const HANDSHAKE_LENGTH: usize = 68;

/// 消息类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Choke - 不再允许对方请求
    Choke = 0,
    /// Unchoke - 允许对方请求
    Unchoke = 1,
    /// Interested - 想要下载对方的数据
    Interested = 2,
    /// Not Interested - 不想下载对方的数据
    NotInterested = 3,
    /// Have - 宣布拥有某个 piece
    Have = 4,
    /// Bitfield - 发送拥有的 pieces 位图
    Bitfield = 5,
    /// Request - 请求一个 block
    Request = 6,
    /// Piece - 发送一个 block
    Piece = 7,
    /// Cancel - 取消请求
    Cancel = 8,
}

impl MessageType {
    /// 从字节解析
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(MessageType::Choke),
            1 => Some(MessageType::Unchoke),
            2 => Some(MessageType::Interested),
            3 => Some(MessageType::NotInterested),
            4 => Some(MessageType::Have),
            5 => Some(MessageType::Bitfield),
            6 => Some(MessageType::Request),
            7 => Some(MessageType::Piece),
            8 => Some(MessageType::Cancel),
            _ => None,
        }
    }
}

/// 握手消息
#[derive(Debug, Clone)]
pub struct Handshake {
    /// Info hash (20 字节)
    pub info_hash: [u8; 20],

    /// Peer ID (20 字节)
    pub peer_id: [u8; 20],

    /// 扩展位（8 字节，用于 DHT/扩展协议）
    pub extensions: [u8; 8],
}

impl Handshake {
    /// 创建新的握手消息
    pub fn new(info_hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Handshake {
            info_hash,
            peer_id,
            extensions: [0; 8], // 暂不使用扩展
        }
    }

    /// 序列化为字节
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HANDSHAKE_LENGTH);

        // 协议字符串长度 (1 字节)
        bytes.push(PROTOCOL_STRING.len() as u8);

        // 协议字符串
        bytes.extend_from_slice(PROTOCOL_STRING);

        // 扩展位
        bytes.extend_from_slice(&self.extensions);

        // Info hash
        bytes.extend_from_slice(&self.info_hash);

        // Peer ID
        bytes.extend_from_slice(&self.peer_id);

        bytes
    }

    /// 从字节解析
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < HANDSHAKE_LENGTH {
            return Err(Error::Other("Invalid handshake length".to_string()));
        }

        let mut cursor = Cursor::new(data);

        // 读取协议字符串长度
        let mut protocol_len = [0u8; 1];
        cursor.read_exact(&mut protocol_len)?;

        // 验证协议字符串
        if protocol_len[0] as usize != PROTOCOL_STRING.len() {
            return Err(Error::Other("Invalid protocol string length".to_string()));
        }

        let mut protocol_bytes = vec![0u8; PROTOCOL_STRING.len()];
        cursor.read_exact(&mut protocol_bytes)?;

        if &protocol_bytes != PROTOCOL_STRING {
            return Err(Error::Other("Invalid protocol string".to_string()));
        }

        // 读取扩展位
        let mut extensions = [0u8; 8];
        cursor.read_exact(&mut extensions)?;

        // 读取 info hash
        let mut info_hash = [0u8; 20];
        cursor.read_exact(&mut info_hash)?;

        // 读取 peer ID
        let mut peer_id = [0u8; 20];
        cursor.read_exact(&mut peer_id)?;

        Ok(Handshake {
            info_hash,
            peer_id,
            extensions,
        })
    }
}

/// 协议消息
#[derive(Debug, Clone)]
pub enum Message {
    /// Choke
    Choke,

    /// Unchoke
    Unchoke,

    /// Interested
    Interested,

    /// Not Interested
    NotInterested,

    /// Have (piece 索引)
    Have { index: usize },

    /// Bitfield (位图)
    Bitfield { bitmap: Vec<u8> },

    /// Request (piece 索引, 偏移, 长度)
    Request {
        index: u32,
        begin: u32,
        length: u32,
    },

    /// Piece (piece 索引, 偏移, 数据)
    Piece {
        index: u32,
        begin: u32,
        block: Vec<u8>,
    },

    /// Cancel (取消请求)
    Cancel {
        index: u32,
        begin: u32,
        length: u32,
    },
}

impl Message {
    /// 序列化消息
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        let message_id: u8;

        match self {
            Message::Choke => {
                message_id = 0;
            }
            Message::Unchoke => {
                message_id = 1;
            }
            Message::Interested => {
                message_id = 2;
            }
            Message::NotInterested => {
                message_id = 3;
            }
            Message::Have { index } => {
                message_id = 4;
                payload.extend_from_slice(&index.to_be_bytes());
            }
            Message::Bitfield { bitmap } => {
                message_id = 5;
                payload.extend_from_slice(bitmap);
            }
            Message::Request {
                index,
                begin,
                length,
            } => {
                message_id = 6;
                payload.extend_from_slice(&index.to_be_bytes());
                payload.extend_from_slice(&begin.to_be_bytes());
                payload.extend_from_slice(&length.to_be_bytes());
            }
            Message::Piece {
                index,
                begin,
                block,
            } => {
                message_id = 7;
                payload.extend_from_slice(&index.to_be_bytes());
                payload.extend_from_slice(&begin.to_be_bytes());
                payload.extend_from_slice(block);
            }
            Message::Cancel {
                index,
                begin,
                length,
            } => {
                message_id = 8;
                payload.extend_from_slice(&index.to_be_bytes());
                payload.extend_from_slice(&begin.to_be_bytes());
                payload.extend_from_slice(&length.to_be_bytes());
            }
        }

        // 前置消息长度 (payload 长度 + 1 字节消息 ID)
        let length = (payload.len() + 1) as u32;

        let mut bytes = Vec::with_capacity(4 + 1 + payload.len());
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.push(message_id);
        bytes.extend_from_slice(&payload);

        bytes
    }

    /// 从字节解析消息
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.is_empty() {
            // Keep-alive 消息
            return Ok(Message::Unchoke); // 作为占位
        }

        let mut cursor = Cursor::new(data);

        // 读取长度
        let mut length_bytes = [0u8; 4];
        cursor.read_exact(&mut length_bytes)?;
        let length = u32::from_be_bytes(length_bytes) as usize;

        // 如果 length == 0，这是 keep-alive
        if length == 0 {
            return Ok(Message::Unchoke); // 占位，实际应该单独处理
        }

        // 读取消息 ID
        let mut message_id_byte = [0u8; 1];
        cursor.read_exact(&mut message_id_byte)?;
        let message_id = message_id_byte[0];

        // 读取 payload
        let payload_length = length - 1;
        let mut payload = vec![0u8; payload_length];
        if payload_length > 0 {
            cursor.read_exact(&mut payload)?;
        }

        // 解析消息
        let message_type = MessageType::from_byte(message_id)
            .ok_or_else(|| Error::Other(format!("Unknown message type: {}", message_id)))?;

        let mut payload_cursor = Cursor::new(payload);

        match message_type {
            MessageType::Choke => Ok(Message::Choke),
            MessageType::Unchoke => Ok(Message::Unchoke),
            MessageType::Interested => Ok(Message::Interested),
            MessageType::NotInterested => Ok(Message::NotInterested),
            MessageType::Have => {
                let mut index_bytes = [0u8; 4];
                payload_cursor.read_exact(&mut index_bytes)?;
                let index = u32::from_be_bytes(index_bytes) as usize;
                Ok(Message::Have { index })
            }
            MessageType::Bitfield => {
                let mut bitmap = vec![0u8; payload_length];
                if payload_length > 0 {
                    payload_cursor.read_exact(&mut bitmap)?;
                }
                Ok(Message::Bitfield { bitmap })
            }
            MessageType::Request => {
                let mut index_bytes = [0u8; 4];
                let mut begin_bytes = [0u8; 4];
                let mut length_bytes = [0u8; 4];

                payload_cursor.read_exact(&mut index_bytes)?;
                payload_cursor.read_exact(&mut begin_bytes)?;
                payload_cursor.read_exact(&mut length_bytes)?;

                Ok(Message::Request {
                    index: u32::from_be_bytes(index_bytes),
                    begin: u32::from_be_bytes(begin_bytes),
                    length: u32::from_be_bytes(length_bytes),
                })
            }
            MessageType::Piece => {
                let mut index_bytes = [0u8; 4];
                let mut begin_bytes = [0u8; 4];

                payload_cursor.read_exact(&mut index_bytes)?;
                payload_cursor.read_exact(&mut begin_bytes)?;

                let index = u32::from_be_bytes(index_bytes);
                let begin = u32::from_be_bytes(begin_bytes);

                let block_size = payload_length - 8;
                let mut block = vec![0u8; block_size];
                if block_size > 0 {
                    payload_cursor.read_exact(&mut block)?;
                }

                Ok(Message::Piece { index, begin, block })
            }
            MessageType::Cancel => {
                let mut index_bytes = [0u8; 4];
                let mut begin_bytes = [0u8; 4];
                let mut length_bytes = [0u8; 4];

                payload_cursor.read_exact(&mut index_bytes)?;
                payload_cursor.read_exact(&mut begin_bytes)?;
                payload_cursor.read_exact(&mut length_bytes)?;

                Ok(Message::Cancel {
                    index: u32::from_be_bytes(index_bytes),
                    begin: u32::from_be_bytes(begin_bytes),
                    length: u32::from_be_bytes(length_bytes),
                })
            }
        }
    }

    /// 获取消息长度（不包括长度前缀）
    pub fn payload_len(&self) -> u32 {
        match self {
            Message::Choke | Message::Unchoke | Message::Interested | Message::NotInterested => 1,
            Message::Have { .. } => 5,
            Message::Bitfield { bitmap } => 1 + bitmap.len() as u32,
            Message::Request { .. } | Message::Cancel { .. } => 13,
            Message::Piece { block, .. } => 9 + block.len() as u32,
        }
    }
}

/// Peer 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerState {
    /// 是否被 choke（对方是否阻止我们请求）
    pub choked: bool,

    /// 是否对对方感兴趣（我们是否想下载对方的数据）
    pub interested: bool,

    /// 对方是否被我们 choke
    pub peer_choked: bool,

    /// 对方是否对我们感兴趣
    pub peer_interested: bool,
}

impl Default for PeerState {
    fn default() -> Self {
        PeerState {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
        }
    }
}

impl PeerState {
    /// 是否可以下载数据（未被 choke 且对方有数据）
    pub fn can_download(&self) -> bool {
        !self.choked
    }

    /// 是否可以上传数据（对方未被 choke）
    pub fn can_upload(&self) -> bool {
        !self.peer_choked
    }

    /// 处理接收到的消息，更新状态
    pub fn handle_message(&mut self, message: &Message) {
        match message {
            Message::Choke => {
                self.choked = true;
            }
            Message::Unchoke => {
                self.choked = false;
            }
            Message::Interested => {
                self.peer_interested = true;
            }
            Message::NotInterested => {
                self.peer_interested = false;
            }
            _ => {}
        }
    }

    /// 创建 Choke 消息
    pub fn choke_message() -> Message {
        Message::Choke
    }

    /// 创建 Unchoke 消息
    pub fn unchoke_message() -> Message {
        Message::Unchoke
    }

    /// 创建 Interested 消息
    pub fn interested_message() -> Message {
        Message::Interested
    }

    /// 创建 Not Interested 消息
    pub fn not_interested_message() -> Message {
        Message::NotInterested
    }
}
