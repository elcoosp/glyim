use glyim_macro_vfs::ContentHash;
use sha2::{Digest, Sha256};

pub const DATA_TYPE_HIR_FN: u8 = 0x01;
pub const DATA_TYPE_HIR_ITEM: u8 = 0x02;
pub const DATA_TYPE_LLVM_FUNCTION: u8 = 0x03;
pub const DATA_TYPE_OBJECT_CODE: u8 = 0x04;

#[derive(Clone, Debug)]
pub struct MerkleNode {
    pub hash: ContentHash,
    pub children: Vec<ContentHash>,
    pub data: MerkleNodeData,
    pub header: MerkleNodeHeader,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MerkleNodeHeader {
    pub data_type_tag: u8,
    pub child_count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MerkleNodeData {
    HirFn { name: String, serialized: Vec<u8> },
    HirItem { kind: String, name: String, serialized: Vec<u8> },
    LlvmFunction { symbol: String, bitcode: Vec<u8> },
    ObjectCode { symbol_name: String, bytes: Vec<u8> },
}

impl MerkleNodeData {
    pub fn data_type_tag(&self) -> u8 {
        match self {
            Self::HirFn { .. } => DATA_TYPE_HIR_FN,
            Self::HirItem { .. } => DATA_TYPE_HIR_ITEM,
            Self::LlvmFunction { .. } => DATA_TYPE_LLVM_FUNCTION,
            Self::ObjectCode { .. } => DATA_TYPE_OBJECT_CODE,
        }
    }
}

impl MerkleNode {
    pub fn compute_hash(&self) -> ContentHash {
        let mut hasher = Sha256::new();
        hasher.update(&(self.children.len() as u64).to_le_bytes());
        for child in &self.children {
            hasher.update(child.as_bytes());
        }
        hasher.update(&self.serialize_data());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        ContentHash::from_bytes(bytes)
    }

    pub fn serialize(&self) -> Vec<u8> {
        // Use postcard to encode the header
        let header_bytes =
            postcard::to_allocvec(&self.header).expect("serialize header");
        let mut buf = Vec::new();
        buf.extend_from_slice(&(header_bytes.len() as u64).to_le_bytes());
        buf.extend_from_slice(&header_bytes);
        // Children hashes (fixed 32 bytes each)
        for child in &self.children {
            buf.extend_from_slice(child.as_bytes());
        }
        // Data payload
        buf.extend_from_slice(&self.serialize_data());
        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NodeDeserializeError> {
        let mut offset = 0;

        // Read header length
        if data.len() < 8 {
            return Err(NodeDeserializeError::TooShort);
        }
        let header_len =
            u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        offset = 8;

        // Read header bytes
        if data.len() < offset + header_len {
            return Err(NodeDeserializeError::TooShort);
        }
        let header: MerkleNodeHeader =
            postcard::from_bytes(&data[offset..offset + header_len])
                .map_err(|e| NodeDeserializeError::HeaderCorrupt(e.to_string()))?;
        offset += header_len;

        // Read child hashes
        let child_count = header.child_count as usize;
        let children_size = child_count * 32;
        if data.len() < offset + children_size {
            return Err(NodeDeserializeError::TooShort);
        }
        let mut children = Vec::with_capacity(child_count);
        for i in 0..child_count {
            let start = offset + i * 32;
            let mut hash_bytes = [0u8; 32];
            hash_bytes.copy_from_slice(&data[start..start + 32]);
            children.push(ContentHash::from_bytes(hash_bytes));
        }
        offset += children_size;

        // Remaining data is the payload blob
        let data_blob = data[offset..].to_vec();
        let merkle_data =
            Self::deserialize_data(header.data_type_tag, &data_blob)?;

        // Recompute hash to verify integrity (optional, but useful)
        let hash = {
            let mut hasher = Sha256::new();
            hasher.update(&(children.len() as u64).to_le_bytes());
            for child in &children {
                hasher.update(child.as_bytes());
            }
            hasher.update(&data_blob);
            let digest = hasher.finalize();
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(&digest);
            ContentHash::from_bytes(bytes)
        };

        Ok(Self {
            hash,
            children,
            data: merkle_data,
            header,
        })
    }

    fn serialize_data(&self) -> Vec<u8> {
        match &self.data {
            MerkleNodeData::HirFn { name, serialized } => {
                let mut buf = vec![DATA_TYPE_HIR_FN];
                buf.extend_from_slice(&(name.len() as u64).to_le_bytes());
                buf.extend_from_slice(name.as_bytes());
                buf.extend_from_slice(&(serialized.len() as u64).to_le_bytes());
                buf.extend_from_slice(serialized);
                buf
            }
            MerkleNodeData::HirItem {
                kind,
                name,
                serialized,
            } => {
                let mut buf = vec![DATA_TYPE_HIR_ITEM];
                buf.extend_from_slice(&(kind.len() as u64).to_le_bytes());
                buf.extend_from_slice(kind.as_bytes());
                buf.extend_from_slice(&(name.len() as u64).to_le_bytes());
                buf.extend_from_slice(name.as_bytes());
                buf.extend_from_slice(&(serialized.len() as u64).to_le_bytes());
                buf.extend_from_slice(serialized);
                buf
            }
            MerkleNodeData::LlvmFunction { symbol, bitcode } => {
                let mut buf = vec![DATA_TYPE_LLVM_FUNCTION];
                buf.extend_from_slice(&(symbol.len() as u64).to_le_bytes());
                buf.extend_from_slice(symbol.as_bytes());
                buf.extend_from_slice(&(bitcode.len() as u64).to_le_bytes());
                buf.extend_from_slice(bitcode);
                buf
            }
            MerkleNodeData::ObjectCode {
                symbol_name,
                bytes,
            } => {
                let mut buf = vec![DATA_TYPE_OBJECT_CODE];
                buf.extend_from_slice(&(symbol_name.len() as u64).to_le_bytes());
                buf.extend_from_slice(symbol_name.as_bytes());
                buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
                buf.extend_from_slice(bytes);
                buf
            }
        }
    }

    fn deserialize_data(
        tag: u8,
        data: &[u8],
    ) -> Result<MerkleNodeData, NodeDeserializeError> {
        let mut offset = 0;
        match tag {
            DATA_TYPE_HIR_FN => {
                let name_len = read_u64(data, &mut offset)? as usize;
                let name = read_string(data, &mut offset, name_len)?;
                let serialized_len = read_u64(data, &mut offset)? as usize;
                let serialized = read_bytes(data, &mut offset, serialized_len)?;
                Ok(MerkleNodeData::HirFn { name, serialized })
            }
            DATA_TYPE_HIR_ITEM => {
                let kind_len = read_u64(data, &mut offset)? as usize;
                let kind = read_string(data, &mut offset, kind_len)?;
                let name_len = read_u64(data, &mut offset)? as usize;
                let name = read_string(data, &mut offset, name_len)?;
                let serialized_len = read_u64(data, &mut offset)? as usize;
                let serialized =
                    read_bytes(data, &mut offset, serialized_len)?;
                Ok(MerkleNodeData::HirItem {
                    kind,
                    name,
                    serialized,
                })
            }
            DATA_TYPE_LLVM_FUNCTION => {
                let symbol_len = read_u64(data, &mut offset)? as usize;
                let symbol = read_string(data, &mut offset, symbol_len)?;
                let bitcode_len = read_u64(data, &mut offset)? as usize;
                let bitcode =
                    read_bytes(data, &mut offset, bitcode_len)?;
                Ok(MerkleNodeData::LlvmFunction { symbol, bitcode })
            }
            DATA_TYPE_OBJECT_CODE => {
                let symbol_name_len =
                    read_u64(data, &mut offset)? as usize;
                let symbol_name =
                    read_string(data, &mut offset, symbol_name_len)?;
                let bytes_len = read_u64(data, &mut offset)? as usize;
                let bytes = read_bytes(data, &mut offset, bytes_len)?;
                Ok(MerkleNodeData::ObjectCode {
                    symbol_name,
                    bytes,
                })
            }
            _ => Err(NodeDeserializeError::UnknownDataType(tag)),
        }
    }
}

// ── helpers ─────────────────────────────────
fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, NodeDeserializeError> {
    if data.len() < *offset + 8 {
        return Err(NodeDeserializeError::TooShort);
    }
    let val =
        u64::from_le_bytes(data[*offset..*offset + 8].try_into().unwrap());
    *offset += 8;
    Ok(val)
}
fn read_string(
    data: &[u8],
    offset: &mut usize,
    len: usize,
) -> Result<String, NodeDeserializeError> {
    if data.len() < *offset + len {
        return Err(NodeDeserializeError::TooShort);
    }
    let s = String::from_utf8(data[*offset..*offset + len].to_vec())
        .map_err(|e| NodeDeserializeError::InvalidUtf8(e.to_string()))?;
    *offset += len;
    Ok(s)
}
fn read_bytes(
    data: &[u8],
    offset: &mut usize,
    len: usize,
) -> Result<Vec<u8>, NodeDeserializeError> {
    if data.len() < *offset + len {
        return Err(NodeDeserializeError::TooShort);
    }
    let bytes = data[*offset..*offset + len].to_vec();
    *offset += len;
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeDeserializeError {
    TooShort,
    HeaderCorrupt(String),
    UnknownDataType(u8),
    InvalidUtf8(String),
}
impl std::fmt::Display for NodeDeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooShort => write!(f, "data too short"),
            Self::HeaderCorrupt(m) => write!(f, "header corrupt: {m}"),
            Self::UnknownDataType(t) => write!(f, "unknown data type tag: {t}"),
            Self::InvalidUtf8(m) => write!(f, "invalid utf8: {m}"),
        }
    }
}
impl std::error::Error for NodeDeserializeError {}
